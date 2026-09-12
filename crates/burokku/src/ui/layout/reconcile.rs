use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
};

use taffy::{geometry::Size, Dimension, Display, Layout, Position, Style};

use crate::ui::{
    elements::{
        styles::position::Position as DomPosition, traits::Styles, Dom, Element,
        NodeId as DomNodeId, NodeKind, NodeRevisions,
    },
    text::{collect_paragraph, ParagraphInput},
};

use super::{
    cache::NodeLayoutCache,
    error::LayoutError,
    topology::{LayoutId, LayoutTopology},
    LogicalViewport, LAYOUT_TREE_DEPTH_LIMIT, LAYOUT_TREE_DEPTH_WARNING,
};

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

impl LayoutNodeState {
    pub(super) fn final_content_width(&self) -> f32 {
        self.unrounded.content_box_width()
    }
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

impl ScratchLayout {
    pub(super) fn visible_paragraph_ids(&self) -> Result<Vec<LayoutId>, LayoutError> {
        visible_paragraph_ids(&self.topology, &self.nodes)
    }
}

pub(super) fn visible_paragraph_ids(
    topology: &LayoutTopology,
    nodes: &HashMap<LayoutId, LayoutNodeState>,
) -> Result<Vec<LayoutId>, LayoutError> {
    let Some(root) = topology.root() else {
        return Ok(Vec::new());
    };

    let mut paragraphs = Vec::new();
    let mut pending = vec![root];
    while let Some(id) = pending.pop() {
        let state = nodes
            .get(&id)
            .ok_or(LayoutError::MissingLayoutSidecar(id))?;
        if state.style.display == Display::None {
            continue;
        }
        if matches!(&state.role, LayoutRole::Paragraph { .. }) {
            paragraphs.push(id);
        }

        let children = topology
            .children(id)
            .ok_or(LayoutError::MissingLayoutNode(id))?;
        pending.extend(children.iter().rev().copied());
    }
    Ok(paragraphs)
}

struct PendingNode {
    dom_id: DomNodeId,
    dom_parent: DomNodeId,
    dom_layout_parent: LayoutId,
    positioned_ancestor: LayoutId,
    parent_container_depth: usize,
    source_order: usize,
}

pub(super) fn reconcile_full(
    dom: &Dom,
    viewport: LogicalViewport,
) -> Result<ScratchLayout, LayoutError> {
    let app = dom.root();
    if !matches!(dom.kind(app), Some(NodeKind::App)) {
        return Err(LayoutError::InvalidAppRoot);
    }

    let app_children = dom.children(app).ok_or(LayoutError::InvalidAppRoot)?;
    if app_children.len() > 1 {
        return Err(LayoutError::InvalidAppChildren {
            count: app_children.len(),
        });
    }

    let mut scratch = ScratchLayout {
        revision: dom.revision(),
        viewport,
        window: None,
        topology: LayoutTopology::default(),
        nodes: HashMap::new(),
        text_owner: HashMap::new(),
    };

    let Some(&window) = app_children.first() else {
        scratch.topology.validate(&HashSet::new())?;
        return Ok(scratch);
    };
    if dom.parent(window) != Some(app) {
        return Err(LayoutError::InvalidDomRelationship {
            parent: app,
            child: window,
        });
    }
    let Some(Element::Window { .. }) = dom.element(window) else {
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
        style_for(dom, window, viewport)?,
        revisions(dom, window)?,
    );
    scratch.window = Some(window);

    let mut pending = Vec::new();
    let mut max_container_depth = 1;
    schedule_children(dom, window, window_layout, window_layout, 1, &mut pending)?;

    while let Some(next) = pending.pop() {
        if !seen_dom.insert(next.dom_id) {
            return Err(LayoutError::DuplicateDomNode(next.dom_id));
        }

        let node = dom
            .node(next.dom_id)
            .ok_or(LayoutError::MissingDomNode(next.dom_id))?;
        let position = match node.kind() {
            NodeKind::Element(element) => element.position(),
            NodeKind::App => return Err(LayoutError::InvalidAppRoot),
            NodeKind::Text(_) => return Err(LayoutError::RawTextOutsideParagraph(next.dom_id)),
        };
        let layout_parent = match position {
            DomPosition::Static | DomPosition::Relative => next.dom_layout_parent,
            DomPosition::Absolute => next.positioned_ancestor,
            DomPosition::Fixed => window_layout,
        };
        match node.kind() {
            NodeKind::App | NodeKind::Text(_) => unreachable!("node kind was checked above"),
            NodeKind::Element(Element::Window { .. }) => {
                return Err(LayoutError::UnexpectedWindow(next.dom_id));
            }
            NodeKind::Element(Element::Text { .. }) => {
                let layout_id = scratch.topology.insert_child(
                    next.dom_id,
                    next.dom_parent,
                    layout_parent,
                    next.source_order,
                )?;
                let collected = collect_paragraph(dom, next.dom_id)?;
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
                    style_for(dom, next.dom_id, viewport)?,
                    node.revisions(),
                );
            }
            NodeKind::Element(
                Element::Div { .. } | Element::Flex { .. } | Element::Grid { .. },
            ) => {
                let container_depth = next.parent_container_depth + 1;
                max_container_depth = max_container_depth.max(container_depth);
                if container_depth > LAYOUT_TREE_DEPTH_LIMIT {
                    return Err(LayoutError::TreeTooDeep {
                        depth: container_depth,
                        limit: LAYOUT_TREE_DEPTH_LIMIT,
                    });
                }
                let layout_id = scratch.topology.insert_child(
                    next.dom_id,
                    next.dom_parent,
                    layout_parent,
                    next.source_order,
                )?;
                insert_state(
                    &mut scratch.nodes,
                    layout_id,
                    next.dom_id,
                    LayoutRole::Container,
                    style_for(dom, next.dom_id, viewport)?,
                    node.revisions(),
                );
                let positioned_ancestor = if position == DomPosition::Static {
                    next.positioned_ancestor
                } else {
                    layout_id
                };
                schedule_children(
                    dom,
                    next.dom_id,
                    layout_id,
                    positioned_ancestor,
                    container_depth,
                    &mut pending,
                )?;
            }
        }
    }

    if max_container_depth > LAYOUT_TREE_DEPTH_WARNING {
        eprintln!(
            "Burokku warning: layout container depth {max_container_depth} exceeds {LAYOUT_TREE_DEPTH_WARNING}",
        );
    }

    let sidecar_ids = scratch.nodes.keys().copied().collect::<HashSet<_>>();
    scratch.topology.validate(&sidecar_ids)?;
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

fn schedule_children(
    dom: &Dom,
    dom_parent: DomNodeId,
    dom_layout_parent: LayoutId,
    positioned_ancestor: LayoutId,
    parent_container_depth: usize,
    pending: &mut Vec<PendingNode>,
) -> Result<(), LayoutError> {
    let children = dom
        .children(dom_parent)
        .ok_or(LayoutError::MissingDomNode(dom_parent))?;
    for (source_order, &child) in children.iter().enumerate().rev() {
        if dom.parent(child) != Some(dom_parent) {
            return Err(LayoutError::InvalidDomRelationship {
                parent: dom_parent,
                child,
            });
        }
        pending.push(PendingNode {
            dom_id: child,
            dom_parent,
            dom_layout_parent,
            positioned_ancestor,
            parent_container_depth,
            source_order,
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

fn revisions(dom: &Dom, id: DomNodeId) -> Result<NodeRevisions, LayoutError> {
    dom.node(id)
        .map(|node| node.revisions())
        .ok_or(LayoutError::MissingDomNode(id))
}

fn style_for(
    dom: &Dom,
    id: DomNodeId,
    viewport: LogicalViewport,
) -> Result<Style<String>, LayoutError> {
    let element = dom.element(id).ok_or(LayoutError::MissingDomNode(id))?;
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
