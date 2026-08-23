use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
};

use taffy::{geometry::Size, Dimension, Layout, Position, Style};

use crate::ui::{
    elements::{
        traits::Styles, ChangeSet, DomSnapshot, Element, NodeId as DomNodeId, NodeKind,
        NodeRevisions, PublishedDom,
    },
    text::{collect_paragraph, ParagraphInput},
};

use super::{
    cache::NodeLayoutCache,
    error::LayoutError,
    topology::{LayoutId, LayoutTopology},
    LogicalViewport,
};

pub(super) const MAX_LAYOUT_DEPTH: usize = 512;

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
        assert!(
            next.depth <= MAX_LAYOUT_DEPTH,
            "layout tree exceeds the supported depth of {MAX_LAYOUT_DEPTH} at node {:?}",
            next.dom_id
        );
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
                let collected =
                    collect_paragraph(snapshot, next.dom_id, next.depth, MAX_LAYOUT_DEPTH)?;
                let (input, descendants) = collected.into_parts();
                scratch.text_owner.insert(next.dom_id, next.dom_id);
                for descendant in descendants {
                    if !seen_dom.insert(descendant) {
                        return Err(LayoutError::DuplicateDomNode(descendant));
                    }
                    scratch.text_owner.insert(descendant, next.dom_id);
                }
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
