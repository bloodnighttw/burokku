use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
};

use taffy::{geometry::Size, Dimension, Layout, Position, Style};

use crate::ui::elements::{
    traits::Styles, ChangeSet, DomSnapshot, Element, NodeId as DomNodeId, NodeKind, NodeRevisions,
    PublishedDom,
};

use super::{
    cache::NodeLayoutCache,
    error::LayoutError,
    topology::{LayoutId, LayoutTopology},
    LogicalViewport,
};

pub(super) const MAX_LAYOUT_DEPTH: usize = 512;

#[derive(Clone, Debug)]
pub(super) struct ParagraphInput {
    source: DomNodeId,
    text: String,
}

impl ParagraphInput {
    pub(super) fn source(&self) -> DomNodeId {
        self.source
    }

    pub(super) fn text(&self) -> &str {
        &self.text
    }
}

#[derive(Clone, Debug)]
pub(super) enum LayoutRole {
    Container,
    Paragraph { input: Rc<ParagraphInput> },
}

#[derive(Clone, Debug)]
pub(super) struct LayoutNodeState {
    pub(super) dom_id: DomNodeId,
    pub(super) role: LayoutRole,
    pub(super) style: Style<String>,
    pub(super) revisions: NodeRevisions,
    pub(super) cache: NodeLayoutCache,
    pub(super) unrounded: Layout,
}

#[derive(Debug)]
pub(super) struct ScratchLayout {
    pub(super) revision: u64,
    pub(super) viewport: LogicalViewport,
    pub(super) window: Option<DomNodeId>,
    pub(super) topology: LayoutTopology,
    pub(super) nodes: HashMap<LayoutId, LayoutNodeState>,
    pub(super) text_owner: HashMap<DomNodeId, DomNodeId>,
}

struct PendingNode {
    dom_id: DomNodeId,
    dom_parent: DomNodeId,
    layout_parent: LayoutId,
    source_order: usize,
    depth: usize,
}

pub(super) fn reconcile_full(
    publication: &PublishedDom,
    viewport: LogicalViewport,
) -> Result<ScratchLayout, LayoutError> {
    validate_publication(publication)?;
    let snapshot = publication.snapshot();
    let app = snapshot.root();
    if !matches!(snapshot.kind(app), Some(NodeKind::App)) {
        return Err(LayoutError::InvalidAppRoot);
    }

    let app_children = snapshot.children(app).ok_or(LayoutError::InvalidAppRoot)?;
    if app_children.len() > 1 {
        return Err(LayoutError::InvalidAppChildren {
            count: app_children.len(),
        });
    }

    let mut scratch = ScratchLayout {
        revision: publication.revision(),
        viewport,
        window: None,
        topology: LayoutTopology::default(),
        nodes: HashMap::new(),
        text_owner: HashMap::new(),
    };

    let Some(&window) = app_children.first() else {
        scratch
            .topology
            .validate(&HashSet::new(), MAX_LAYOUT_DEPTH)?;
        return Ok(scratch);
    };
    if snapshot.parent(window) != Some(app) {
        return Err(LayoutError::InvalidDomRelationship {
            parent: app,
            child: window,
        });
    }
    let Some(Element::Window { .. }) = snapshot.element(window) else {
        return Err(LayoutError::ExpectedWindow(window));
    };

    let mut seen_dom = HashSet::new();
    seen_dom.insert(app);
    if !seen_dom.insert(window) {
        return Err(LayoutError::DuplicateDomNode(window));
    }

    let window_layout = scratch.topology.insert_root(window)?;
    insert_state(
        &mut scratch.nodes,
        window_layout,
        window,
        LayoutRole::Container,
        style_for(snapshot, window, viewport)?,
        revisions(snapshot, window)?,
    );
    scratch.window = Some(window);

    let mut pending = Vec::new();
    schedule_children(snapshot, window, window_layout, 1, &mut pending)?;

    while let Some(next) = pending.pop() {
        if next.depth > MAX_LAYOUT_DEPTH {
            return Err(LayoutError::TreeTooDeep {
                node: next.dom_id,
                limit: MAX_LAYOUT_DEPTH,
            });
        }
        if !seen_dom.insert(next.dom_id) {
            return Err(LayoutError::DuplicateDomNode(next.dom_id));
        }

        let node = snapshot
            .node(next.dom_id)
            .ok_or(LayoutError::MissingDomNode(next.dom_id))?;
        match node.kind() {
            NodeKind::App => return Err(LayoutError::InvalidAppRoot),
            NodeKind::Text(_) => return Err(LayoutError::RawTextOutsideParagraph(next.dom_id)),
            NodeKind::Element(Element::Window { .. }) => {
                return Err(LayoutError::UnexpectedWindow(next.dom_id));
            }
            NodeKind::Element(Element::Text { .. }) => {
                let layout_id = scratch.topology.insert_child(
                    next.dom_id,
                    next.dom_parent,
                    next.layout_parent,
                    next.source_order,
                )?;
                let input = collect_paragraph(
                    snapshot,
                    next.dom_id,
                    next.depth,
                    &mut seen_dom,
                    &mut scratch.text_owner,
                )?;
                insert_state(
                    &mut scratch.nodes,
                    layout_id,
                    next.dom_id,
                    LayoutRole::Paragraph {
                        input: Rc::new(input),
                    },
                    style_for(snapshot, next.dom_id, viewport)?,
                    node.revisions(),
                );
            }
            NodeKind::Element(
                Element::Div { .. } | Element::Flex { .. } | Element::Grid { .. },
            ) => {
                let layout_id = scratch.topology.insert_child(
                    next.dom_id,
                    next.dom_parent,
                    next.layout_parent,
                    next.source_order,
                )?;
                insert_state(
                    &mut scratch.nodes,
                    layout_id,
                    next.dom_id,
                    LayoutRole::Container,
                    style_for(snapshot, next.dom_id, viewport)?,
                    node.revisions(),
                );
                schedule_children(
                    snapshot,
                    next.dom_id,
                    layout_id,
                    next.depth + 1,
                    &mut pending,
                )?;
            }
        }
    }

    let sidecar_ids = scratch.nodes.keys().copied().collect::<HashSet<_>>();
    scratch.topology.validate(&sidecar_ids, MAX_LAYOUT_DEPTH)?;
    for (&id, state) in &scratch.nodes {
        if matches!(state.role, LayoutRole::Paragraph { .. })
            && !scratch
                .topology
                .children(id)
                .ok_or(LayoutError::MissingLayoutNode(id))?
                .is_empty()
        {
            return Err(LayoutError::InvalidParagraphChild {
                paragraph: state.dom_id,
                child: state.dom_id,
            });
        }
    }

    Ok(scratch)
}

fn validate_publication(publication: &PublishedDom) -> Result<(), LayoutError> {
    let snapshot_revision = publication.snapshot().revision();
    let target_revision = match publication.changes() {
        ChangeSet::FullRebuild { to_revision, .. } => to_revision,
    };
    if snapshot_revision != target_revision {
        return Err(LayoutError::PublicationRevisionMismatch {
            snapshot_revision,
            target_revision,
        });
    }
    Ok(())
}

fn schedule_children(
    snapshot: &DomSnapshot,
    dom_parent: DomNodeId,
    layout_parent: LayoutId,
    depth: usize,
    pending: &mut Vec<PendingNode>,
) -> Result<(), LayoutError> {
    let children = snapshot
        .children(dom_parent)
        .ok_or(LayoutError::MissingDomNode(dom_parent))?;
    for (source_order, &child) in children.iter().enumerate().rev() {
        if snapshot.parent(child) != Some(dom_parent) {
            return Err(LayoutError::InvalidDomRelationship {
                parent: dom_parent,
                child,
            });
        }
        pending.push(PendingNode {
            dom_id: child,
            dom_parent,
            layout_parent,
            source_order,
            depth,
        });
    }
    Ok(())
}

fn collect_paragraph(
    snapshot: &DomSnapshot,
    source: DomNodeId,
    source_depth: usize,
    seen_dom: &mut HashSet<DomNodeId>,
    text_owner: &mut HashMap<DomNodeId, DomNodeId>,
) -> Result<ParagraphInput, LayoutError> {
    text_owner.insert(source, source);
    let children = snapshot
        .children(source)
        .ok_or(LayoutError::MissingDomNode(source))?;
    let mut pending = children
        .iter()
        .enumerate()
        .rev()
        .map(|(_, &id)| (id, source, source_depth + 1))
        .collect::<Vec<_>>();
    let mut text = String::new();

    while let Some((id, parent, depth)) = pending.pop() {
        if depth > MAX_LAYOUT_DEPTH {
            return Err(LayoutError::TreeTooDeep {
                node: id,
                limit: MAX_LAYOUT_DEPTH,
            });
        }
        if snapshot.parent(id) != Some(parent) {
            return Err(LayoutError::InvalidDomRelationship { parent, child: id });
        }
        if !seen_dom.insert(id) {
            return Err(LayoutError::DuplicateDomNode(id));
        }
        text_owner.insert(id, source);

        let node = snapshot.node(id).ok_or(LayoutError::MissingDomNode(id))?;
        match node.kind() {
            NodeKind::Text(value) => text.push_str(value),
            NodeKind::Element(Element::Text { .. }) => {
                pending.extend(
                    node.children()
                        .iter()
                        .enumerate()
                        .rev()
                        .map(|(_, &child)| (child, id, depth + 1)),
                );
            }
            NodeKind::App | NodeKind::Element(_) => {
                return Err(LayoutError::InvalidParagraphChild {
                    paragraph: source,
                    child: id,
                });
            }
        }
    }

    Ok(ParagraphInput { source, text })
}

fn insert_state(
    nodes: &mut HashMap<LayoutId, LayoutNodeState>,
    layout_id: LayoutId,
    dom_id: DomNodeId,
    role: LayoutRole,
    style: Style<String>,
    revisions: NodeRevisions,
) {
    let previous = nodes.insert(
        layout_id,
        LayoutNodeState {
            dom_id,
            role,
            style,
            revisions,
            cache: NodeLayoutCache::default(),
            unrounded: Layout::new(),
        },
    );
    debug_assert!(previous.is_none(), "layout IDs are unique after lowering");
}

fn revisions(snapshot: &DomSnapshot, id: DomNodeId) -> Result<NodeRevisions, LayoutError> {
    snapshot
        .node(id)
        .map(|node| node.revisions())
        .ok_or(LayoutError::MissingDomNode(id))
}

fn style_for(
    snapshot: &DomSnapshot,
    id: DomNodeId,
    viewport: LogicalViewport,
) -> Result<Style<String>, LayoutError> {
    let element = snapshot
        .element(id)
        .ok_or(LayoutError::MissingDomNode(id))?;
    let mut style = match element {
        Element::Window { style } => style.to_taffy_style(),
        Element::Div { style } => style.to_taffy_style(),
        Element::Flex { style } => style.to_taffy_style(),
        Element::Grid { style } => style.to_taffy_style(),
        Element::Text { style } => style.to_taffy_style(),
    };

    if matches!(element, Element::Window { .. }) {
        let viewport_size = Size {
            width: Dimension::length(viewport.width()),
            height: Dimension::length(viewport.height()),
        };
        style.display = taffy::Display::Block;
        style.position = Position::Relative;
        style.size = viewport_size;
        style.min_size = viewport_size;
        style.max_size = viewport_size;
    }
    Ok(style)
}
