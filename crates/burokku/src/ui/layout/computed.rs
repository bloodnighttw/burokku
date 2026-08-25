use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::Arc,
};

use taffy::{geometry::Point, Layout};

use crate::ui::{
    elements::{NodeId as DomNodeId, PublishedDom},
    text::{ShapedParagraph, TextConstraint},
};

use super::{
    error::LayoutError,
    reconcile::{LayoutNodeState, LayoutRole, ScratchLayout},
    topology::{LayoutId, LayoutTopology},
    LogicalViewport,
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct ComputedBox {
    layout: Layout,
    layout_parent: Option<DomNodeId>,
    border_origin: Point<f32>,
    content_origin: Point<f32>,
}

impl ComputedBox {
    pub(crate) fn layout(self) -> Layout {
        self.layout
    }

    pub(crate) fn layout_parent(self) -> Option<DomNodeId> {
        self.layout_parent
    }

    pub(crate) fn border_origin(self) -> Point<f32> {
        self.border_origin
    }

    pub(crate) fn content_origin(self) -> Point<f32> {
        self.content_origin
    }
}

#[derive(Debug)]
pub(crate) struct ComputedLayout {
    publication: Arc<PublishedDom>,
    revision: u64,
    viewport: LogicalViewport,
    text_generation: u64,
    window: Option<DomNodeId>,
    topology: LayoutTopology,
    nodes: HashMap<LayoutId, LayoutNodeState>,
    boxes: HashMap<DomNodeId, ComputedBox>,
    text_owner: HashMap<DomNodeId, DomNodeId>,
    final_paragraphs: HashMap<DomNodeId, Rc<ShapedParagraph>>,
}

impl ComputedLayout {
    pub(super) fn from_scratch(
        publication: Arc<PublishedDom>,
        scratch: ScratchLayout,
        text_generation: u64,
        final_paragraphs: HashMap<DomNodeId, Rc<ShapedParagraph>>,
    ) -> Result<Self, LayoutError> {
        let boxes = build_boxes(&scratch)?;
        validate_final_paragraphs(&scratch, &final_paragraphs)?;
        Ok(Self {
            revision: scratch.revision,
            viewport: scratch.viewport,
            text_generation,
            window: scratch.window,
            topology: scratch.topology,
            nodes: scratch.nodes,
            text_owner: scratch.text_owner,
            publication,
            boxes,
            final_paragraphs,
        })
    }

    pub(crate) fn publication(&self) -> &Arc<PublishedDom> {
        &self.publication
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn viewport(&self) -> LogicalViewport {
        self.viewport
    }

    pub(crate) fn text_generation(&self) -> u64 {
        self.text_generation
    }

    pub(crate) fn window(&self) -> Option<DomNodeId> {
        self.window
    }

    pub(crate) fn box_for(&self, id: DomNodeId) -> Option<&ComputedBox> {
        self.boxes.get(&id)
    }

    pub(crate) fn len(&self) -> usize {
        self.boxes.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.boxes.is_empty()
    }

    pub(crate) fn text_owner(&self, id: DomNodeId) -> Option<DomNodeId> {
        self.text_owner.get(&id).copied()
    }

    pub(crate) fn final_paragraph(&self, source: DomNodeId) -> Option<&Rc<ShapedParagraph>> {
        self.final_paragraphs.get(&source)
    }

    pub(crate) fn layout_children(&self, id: DomNodeId) -> Option<Vec<DomNodeId>> {
        let layout_id = self.topology.layout_id(id)?;
        Some(
            self.topology
                .children(layout_id)?
                .iter()
                .filter_map(|child| self.topology.dom_id(*child))
                .collect(),
        )
    }
}

fn validate_final_paragraphs(
    scratch: &ScratchLayout,
    final_paragraphs: &HashMap<DomNodeId, Rc<ShapedParagraph>>,
) -> Result<(), LayoutError> {
    let paragraph_ids = scratch.visible_paragraph_ids()?;
    let mut expected_sources = HashSet::with_capacity(paragraph_ids.len());
    for layout_id in paragraph_ids {
        let state = scratch
            .nodes
            .get(&layout_id)
            .ok_or(LayoutError::MissingLayoutSidecar(layout_id))?;
        let LayoutRole::Paragraph { input } = &state.role else {
            return Err(LayoutError::InvalidFinalParagraph(state.dom_id));
        };
        let source = state.dom_id;
        expected_sources.insert(source);
        let paragraph = final_paragraphs
            .get(&source)
            .ok_or(LayoutError::InvalidFinalParagraph(source))?;
        let expected_constraint = TextConstraint::definite(state.final_content_width())
            .map_err(|_| LayoutError::InvalidFinalParagraph(source))?;
        if paragraph.source() != source
            || paragraph.fingerprint() != input.fingerprint()
            || paragraph.constraint() != expected_constraint
        {
            return Err(LayoutError::InvalidFinalParagraph(source));
        }
    }

    if let Some(source) = final_paragraphs
        .keys()
        .find(|source| !expected_sources.contains(source))
        .copied()
    {
        return Err(LayoutError::InvalidFinalParagraph(source));
    }
    Ok(())
}

fn build_boxes(scratch: &ScratchLayout) -> Result<HashMap<DomNodeId, ComputedBox>, LayoutError> {
    let mut boxes = HashMap::with_capacity(scratch.nodes.len());
    let Some(root) = scratch.topology.root() else {
        return Ok(boxes);
    };

    let mut pending = vec![(root, Point::ZERO)];
    while let Some((id, parent_origin)) = pending.pop() {
        let state = scratch
            .nodes
            .get(&id)
            .ok_or(LayoutError::MissingLayoutSidecar(id))?;
        validate_layout(state.dom_id, &state.unrounded)?;

        let layout = state.unrounded;
        let border_origin = Point {
            x: parent_origin.x + layout.location.x,
            y: parent_origin.y + layout.location.y,
        };
        let content_origin = Point {
            x: border_origin.x + layout.border.left + layout.padding.left,
            y: border_origin.y + layout.border.top + layout.padding.top,
        };
        validate_point(state.dom_id, "border origin", border_origin)?;
        validate_point(state.dom_id, "content origin", content_origin)?;

        let layout_parent = scratch
            .topology
            .parent(id)
            .map(|parent| {
                scratch
                    .topology
                    .dom_id(parent)
                    .ok_or(LayoutError::MissingLayoutNode(parent))
            })
            .transpose()?;
        boxes.insert(
            state.dom_id,
            ComputedBox {
                layout,
                layout_parent,
                border_origin,
                content_origin,
            },
        );

        let children = scratch
            .topology
            .children(id)
            .ok_or(LayoutError::MissingLayoutNode(id))?;
        pending.extend(children.iter().rev().map(|child| (*child, border_origin)));
    }

    if boxes.len() != scratch.nodes.len() {
        if let Some(id) = scratch
            .nodes
            .keys()
            .find(|id| {
                scratch
                    .topology
                    .dom_id(**id)
                    .is_some_and(|dom_id| !boxes.contains_key(&dom_id))
            })
            .copied()
        {
            return Err(LayoutError::UnreachableLayoutNode(id));
        }
    }
    Ok(boxes)
}

fn validate_layout(node: DomNodeId, layout: &Layout) -> Result<(), LayoutError> {
    validate_non_negative(node, "width", layout.size.width)?;
    validate_non_negative(node, "height", layout.size.height)?;
    validate_finite(node, "x", layout.location.x)?;
    validate_finite(node, "y", layout.location.y)?;
    validate_non_negative(node, "scrollbar width", layout.scrollbar_size.width)?;
    validate_non_negative(node, "scrollbar height", layout.scrollbar_size.height)?;
    validate_non_negative(node, "content width", layout.content_size.width)?;
    validate_non_negative(node, "content height", layout.content_size.height)?;

    for (field, value) in [
        ("border left", layout.border.left),
        ("border right", layout.border.right),
        ("border top", layout.border.top),
        ("border bottom", layout.border.bottom),
        ("padding left", layout.padding.left),
        ("padding right", layout.padding.right),
        ("padding top", layout.padding.top),
        ("padding bottom", layout.padding.bottom),
        ("margin left", layout.margin.left),
        ("margin right", layout.margin.right),
        ("margin top", layout.margin.top),
        ("margin bottom", layout.margin.bottom),
    ] {
        validate_finite(node, field, value)?;
    }
    Ok(())
}

fn validate_point(
    node: DomNodeId,
    field: &'static str,
    point: Point<f32>,
) -> Result<(), LayoutError> {
    if !point.x.is_finite() {
        return Err(LayoutError::InvalidComputedValue {
            node,
            field,
            value: point.x,
        });
    }
    if !point.y.is_finite() {
        return Err(LayoutError::InvalidComputedValue {
            node,
            field,
            value: point.y,
        });
    }
    Ok(())
}

fn validate_non_negative(
    node: DomNodeId,
    field: &'static str,
    value: f32,
) -> Result<(), LayoutError> {
    validate_finite(node, field, value)?;
    if value < 0.0 {
        return Err(LayoutError::InvalidComputedValue { node, field, value });
    }
    Ok(())
}

fn validate_finite(node: DomNodeId, field: &'static str, value: f32) -> Result<(), LayoutError> {
    if !value.is_finite() {
        return Err(LayoutError::InvalidComputedValue { node, field, value });
    }
    Ok(())
}
