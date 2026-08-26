use std::{collections::HashMap, rc::Rc};

use taffy::util::ResolveOrZero;
use taffy::{
    compute_block_layout, compute_cached_layout, compute_flexbox_layout, compute_grid_layout,
    compute_hidden_layout, compute_leaf_layout,
    geometry::{Point, Size},
    AvailableSpace, BlockContext, CacheTree, Display, Layout, LayoutBlockContainer,
    LayoutFlexboxContainer, LayoutGridContainer, LayoutInput, LayoutOutput, LayoutPartialTree,
    Overflow, Position, RunMode, Style, TraversePartialTree, TraverseTree,
};

use crate::ui::{
    elements::NodeId as DomNodeId,
    text::{ParagraphInput, ShapedParagraph, TextConstraint},
};

use super::{
    error::LayoutError,
    reconcile::{visible_paragraph_ids, LayoutNodeState, LayoutRole, ScratchLayout},
    topology::{LayoutId, LayoutTopology},
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct TextMeasureRequest<'a> {
    paragraph: &'a ParagraphInput,
    known_dimensions: Size<Option<f32>>,
    available_space: Size<AvailableSpace>,
    final_paragraph_resolution: bool,
}

impl<'a> TextMeasureRequest<'a> {
    pub(crate) fn source(self) -> DomNodeId {
        self.paragraph.source()
    }

    pub(crate) fn text(self) -> &'a str {
        self.paragraph.text()
    }

    pub(crate) fn paragraph(self) -> &'a ParagraphInput {
        self.paragraph
    }

    pub(crate) fn known_dimensions(self) -> Size<Option<f32>> {
        self.known_dimensions
    }

    pub(crate) fn available_space(self) -> Size<AvailableSpace> {
        self.available_space
    }

    /// Whether this request resolves the paint paragraph after Taffy has
    /// completed the node's unrounded layout.
    pub(crate) fn is_final_paragraph_resolution(self) -> bool {
        self.final_paragraph_resolution
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TextMeasurement {
    size: Size<f32>,
    first_baseline: Option<f32>,
    shaped: Option<Rc<ShapedParagraph>>,
}

impl TextMeasurement {
    pub(crate) fn new(size: Size<f32>, first_baseline: Option<f32>) -> Self {
        Self {
            size,
            first_baseline,
            shaped: None,
        }
    }

    pub(crate) fn with_shaped(
        size: Size<f32>,
        first_baseline: Option<f32>,
        shaped: Rc<ShapedParagraph>,
    ) -> Self {
        Self {
            size,
            first_baseline,
            shaped: Some(shaped),
        }
    }

    pub(crate) fn size(&self) -> Size<f32> {
        self.size
    }

    pub(crate) fn first_baseline(&self) -> Option<f32> {
        self.first_baseline
    }

    pub(crate) fn shaped(&self) -> Option<&Rc<ShapedParagraph>> {
        self.shaped.as_ref()
    }
}

pub(crate) trait TextMeasurer {
    /// Generation of external measurement inputs such as fonts or shaping
    /// configuration. Changing it invalidates an otherwise unchanged frame.
    fn generation(&self) -> u64 {
        0
    }

    /// Measure one paragraph request. Final paragraph-resolution requests must
    /// return the exact shaped variant through [`TextMeasurement::with_shaped`].
    fn measure(&mut self, request: TextMeasureRequest<'_>) -> Result<TextMeasurement, String>;

    /// Drop persistent shaping state for paragraph sources absent from the
    /// latest successfully computed frame.
    fn retain_sources(&mut self, _sources: &std::collections::HashSet<DomNodeId>) {}
}

pub(super) fn compute_layout<M: TextMeasurer>(
    scratch: &mut ScratchLayout,
    measurer: &mut M,
) -> Result<HashMap<DomNodeId, Rc<ShapedParagraph>>, LayoutError> {
    let Some(root) = scratch.topology.root() else {
        return Ok(HashMap::new());
    };
    let available_space = Size {
        width: AvailableSpace::Definite(scratch.viewport.width()),
        height: AvailableSpace::Definite(scratch.viewport.height()),
    };
    let mut tree = DerivedLayoutTree {
        topology: &scratch.topology,
        nodes: &mut scratch.nodes,
        measurer,
        first_error: None,
    };
    taffy::compute_root_layout(&mut tree, root.into_taffy(), available_space);
    if let Some(error) = tree.first_error.take() {
        return Err(error);
    }

    // CSS resolves percentage padding on every side against the containing
    // block's inline size. Taffy's block algorithm currently overwrites block
    // child metadata after resolving vertical sides against the block's height,
    // so correct only those two percentage fields. Flex, grid, absolute, fixed-
    // length, and horizontal padding metadata must remain untouched.
    tree.normalize_block_child_vertical_percentage_padding()?;

    // Measurement callbacks are speculative and may be skipped by exact cache
    // hits. Resolve paint paragraphs only from the completed unrounded boxes.
    tree.resolve_final_paragraphs()
}

struct LayoutChildIter<'a>(std::slice::Iter<'a, LayoutId>);

impl Iterator for LayoutChildIter<'_> {
    type Item = taffy::NodeId;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().copied().map(LayoutId::into_taffy)
    }
}

struct DerivedLayoutTree<'a, M> {
    topology: &'a LayoutTopology,
    nodes: &'a mut std::collections::HashMap<LayoutId, LayoutNodeState>,
    measurer: &'a mut M,
    first_error: Option<LayoutError>,
}

impl<M> DerivedLayoutTree<'_, M> {
    fn node(&self, id: taffy::NodeId) -> &LayoutNodeState {
        let id = LayoutId::from_taffy(id);
        self.nodes
            .get(&id)
            .expect("prevalidated layout IDs have sidecars")
    }

    fn node_mut(&mut self, id: taffy::NodeId) -> &mut LayoutNodeState {
        let id = LayoutId::from_taffy(id);
        self.nodes
            .get_mut(&id)
            .expect("prevalidated layout IDs have sidecars")
    }
}

impl<M: TextMeasurer> DerivedLayoutTree<'_, M> {
    fn normalize_block_child_vertical_percentage_padding(&mut self) -> Result<(), LayoutError> {
        let Some(root) = self.topology.root() else {
            return Ok(());
        };
        let mut pending = vec![root];

        while let Some(parent) = pending.pop() {
            let (display, content_width) = {
                let state = self
                    .nodes
                    .get(&parent)
                    .ok_or(LayoutError::MissingLayoutSidecar(parent))?;
                (state.style.display, state.unrounded.content_box_width())
            };
            if display == Display::None {
                continue;
            }

            let children = self
                .topology
                .children(parent)
                .ok_or(LayoutError::MissingLayoutNode(parent))?;
            pending.extend(children.iter().rev().copied());
            if display != Display::Block {
                continue;
            }

            for child in children {
                let state = self
                    .nodes
                    .get_mut(child)
                    .ok_or(LayoutError::MissingLayoutSidecar(*child))?;
                if state.style.display == Display::None
                    || state.style.position == Position::Absolute
                {
                    continue;
                }

                let padding = state.style.padding;
                if padding.top.into_raw().uses_percentage() {
                    state.unrounded.padding.top =
                        padding.top.resolve_or_zero(Some(content_width), |_, _| 0.0);
                }
                if padding.bottom.into_raw().uses_percentage() {
                    state.unrounded.padding.bottom = padding
                        .bottom
                        .resolve_or_zero(Some(content_width), |_, _| 0.0);
                }
            }
        }

        Ok(())
    }

    fn compute_node_layout(
        &mut self,
        node_id: taffy::NodeId,
        input: LayoutInput,
        block_context: Option<&mut BlockContext<'_>>,
    ) -> LayoutOutput {
        if input.run_mode == RunMode::PerformHiddenLayout {
            return compute_hidden_layout(self, node_id);
        }

        compute_cached_layout(self, node_id, input, |tree, node_id, input| {
            let (display, paragraph) = {
                let state = tree.node(node_id);
                let paragraph = match &state.role {
                    LayoutRole::Container => None,
                    LayoutRole::Paragraph { input } => Some(Rc::clone(input)),
                };
                (state.style.display, paragraph)
            };
            let has_children = tree.child_count(node_id) > 0;

            match (display, paragraph, has_children) {
                (Display::None, _, _) => compute_hidden_layout(tree, node_id),
                (_, Some(paragraph), _) => tree.compute_paragraph_leaf(node_id, input, paragraph),
                (Display::Block, None, true) => {
                    compute_block_layout(tree, node_id, input, block_context)
                }
                (Display::Flex, None, true) => compute_flexbox_layout(tree, node_id, input),
                (Display::Grid, None, true) => compute_grid_layout(tree, node_id, input),
                (_, None, false) => {
                    let style = tree.node(node_id).style.clone();
                    compute_leaf_layout(input, &style, |_, _| 0.0, |_, _| Size::ZERO)
                }
            }
        })
    }

    fn compute_paragraph_leaf(
        &mut self,
        node_id: taffy::NodeId,
        input: LayoutInput,
        paragraph: Rc<ParagraphInput>,
    ) -> LayoutOutput {
        let layout_id = LayoutId::from_taffy(node_id);
        let style = self
            .nodes
            .get(&layout_id)
            .expect("the paragraph sidecar is prevalidated")
            .style
            .clone();

        let mut output = compute_leaf_layout(
            input,
            &style,
            |_, _| 0.0,
            |known_dimensions, available_space| {
                self.measure_paragraph(&paragraph, known_dimensions, available_space, false)
                    .map_or(Size::ZERO, |measurement| measurement.size())
            },
        );

        if self.first_error.is_some() {
            return output;
        }

        let padding = style
            .padding
            .resolve_or_zero(input.parent_size.width, |_, _| 0.0);
        let border = style
            .border
            .resolve_or_zero(input.parent_size.width, |_, _| 0.0);
        let scrollbar_width = if style.overflow.y == Overflow::Scroll {
            style.scrollbar_width
        } else {
            0.0
        };
        let content_width = (output.size.width
            - padding.left
            - padding.right
            - border.left
            - border.right
            - scrollbar_width)
            .max(0.0);
        let baseline_measurement = self.measure_paragraph(
            &paragraph,
            Size {
                width: Some(content_width),
                height: None,
            },
            Size {
                width: AvailableSpace::Definite(content_width),
                height: input.available_space.height,
            },
            false,
        );
        if let Some(measurement) = baseline_measurement {
            output.first_baselines = Point {
                x: None,
                y: measurement
                    .first_baseline()
                    .map(|baseline| border.top + padding.top + baseline),
            };
        }
        output
    }

    fn resolve_final_paragraphs(
        &mut self,
    ) -> Result<HashMap<DomNodeId, Rc<ShapedParagraph>>, LayoutError> {
        let paragraph_ids = visible_paragraph_ids(self.topology, self.nodes)?;
        let mut final_paragraphs = HashMap::with_capacity(paragraph_ids.len());
        for layout_id in paragraph_ids {
            let (source, paragraph, content_width) = {
                let state = self
                    .nodes
                    .get(&layout_id)
                    .ok_or(LayoutError::MissingLayoutSidecar(layout_id))?;
                let LayoutRole::Paragraph { input } = &state.role else {
                    return Err(LayoutError::InvalidFinalParagraph(state.dom_id));
                };
                (state.dom_id, Rc::clone(input), state.final_content_width())
            };

            let Some(measurement) = self.measure_paragraph(
                &paragraph,
                Size {
                    width: Some(content_width),
                    height: None,
                },
                Size {
                    width: AvailableSpace::Definite(content_width),
                    height: AvailableSpace::MaxContent,
                },
                true,
            ) else {
                return Err(self
                    .first_error
                    .take()
                    .unwrap_or(LayoutError::InvalidFinalParagraph(source)));
            };
            let Some(shaped) = measurement.shaped().cloned() else {
                return Err(LayoutError::InvalidFinalParagraph(source));
            };
            let expected_constraint = TextConstraint::definite(content_width)
                .map_err(|_| LayoutError::InvalidFinalParagraph(source))?;
            if shaped.source() != source
                || shaped.fingerprint() != paragraph.fingerprint()
                || shaped.constraint() != expected_constraint
                || final_paragraphs.insert(source, shaped).is_some()
            {
                return Err(LayoutError::InvalidFinalParagraph(source));
            }
        }
        Ok(final_paragraphs)
    }

    fn measure_paragraph(
        &mut self,
        paragraph: &ParagraphInput,
        known_dimensions: Size<Option<f32>>,
        available_space: Size<AvailableSpace>,
        final_paragraph_resolution: bool,
    ) -> Option<TextMeasurement> {
        if self.first_error.is_some() {
            return None;
        }

        let known_dimensions = match normalize_known_dimensions(known_dimensions) {
            Ok(dimensions) => dimensions,
            Err((field, value)) => {
                self.first_error = Some(LayoutError::InvalidTextMetric {
                    paragraph: paragraph.source(),
                    field,
                    value,
                });
                return None;
            }
        };
        let available_space = match normalize_available_space(available_space) {
            Ok(space) => space,
            Err((field, value)) => {
                self.first_error = Some(LayoutError::InvalidTextMetric {
                    paragraph: paragraph.source(),
                    field,
                    value,
                });
                return None;
            }
        };
        let request = TextMeasureRequest {
            paragraph,
            known_dimensions,
            available_space,
            final_paragraph_resolution,
        };
        let measurement = match self.measurer.measure(request) {
            Ok(measurement) => measurement,
            Err(message) => {
                self.first_error = Some(LayoutError::TextMeasurement {
                    paragraph: paragraph.source(),
                    message,
                });
                return None;
            }
        };

        for (field, value) in [
            ("width", measurement.size.width),
            ("height", measurement.size.height),
        ] {
            if !value.is_finite() || value < 0.0 {
                self.first_error = Some(LayoutError::InvalidTextMetric {
                    paragraph: paragraph.source(),
                    field,
                    value,
                });
                return None;
            }
        }
        if let Some(baseline) = measurement.first_baseline {
            if !baseline.is_finite() || baseline < 0.0 {
                self.first_error = Some(LayoutError::InvalidTextMetric {
                    paragraph: paragraph.source(),
                    field: "first baseline",
                    value: baseline,
                });
                return None;
            }
        }
        Some(measurement)
    }
}

fn normalize_known_dimensions(
    dimensions: Size<Option<f32>>,
) -> Result<Size<Option<f32>>, (&'static str, f32)> {
    fn normalize(
        value: Option<f32>,
        field: &'static str,
    ) -> Result<Option<f32>, (&'static str, f32)> {
        match value {
            Some(value) if !value.is_finite() || value < 0.0 => Err((field, value)),
            Some(value) => Ok(Some(if value == 0.0 { 0.0 } else { value })),
            None => Ok(None),
        }
    }

    Ok(Size {
        width: normalize(dimensions.width, "known width")?,
        height: normalize(dimensions.height, "known height")?,
    })
}

fn normalize_available_space(
    available_space: Size<AvailableSpace>,
) -> Result<Size<AvailableSpace>, (&'static str, f32)> {
    fn normalize(
        value: AvailableSpace,
        field: &'static str,
    ) -> Result<AvailableSpace, (&'static str, f32)> {
        match value {
            AvailableSpace::Definite(value) if !value.is_finite() => Err((field, value)),
            AvailableSpace::Definite(value) => Ok(AvailableSpace::Definite(value.max(0.0))),
            AvailableSpace::MinContent => Ok(AvailableSpace::MinContent),
            AvailableSpace::MaxContent => Ok(AvailableSpace::MaxContent),
        }
    }

    Ok(Size {
        width: normalize(available_space.width, "available width")?,
        height: normalize(available_space.height, "available height")?,
    })
}

impl<M> TraversePartialTree for DerivedLayoutTree<'_, M> {
    type ChildIter<'a>
        = LayoutChildIter<'a>
    where
        Self: 'a;

    fn child_ids(&self, parent_node_id: taffy::NodeId) -> Self::ChildIter<'_> {
        let id = LayoutId::from_taffy(parent_node_id);
        LayoutChildIter(
            self.topology
                .children(id)
                .expect("prevalidated layout IDs have child lists")
                .iter(),
        )
    }

    fn child_count(&self, parent_node_id: taffy::NodeId) -> usize {
        let id = LayoutId::from_taffy(parent_node_id);
        self.topology
            .children(id)
            .expect("prevalidated layout IDs have child lists")
            .len()
    }

    fn get_child_id(&self, parent_node_id: taffy::NodeId, child_index: usize) -> taffy::NodeId {
        let id = LayoutId::from_taffy(parent_node_id);
        self.topology
            .children(id)
            .expect("prevalidated layout IDs have child lists")[child_index]
            .into_taffy()
    }
}

impl<M> TraverseTree for DerivedLayoutTree<'_, M> {}

impl<M> CacheTree for DerivedLayoutTree<'_, M> {
    fn cache_get(&self, node_id: taffy::NodeId, input: &LayoutInput) -> Option<LayoutOutput> {
        if self.first_error.is_some() {
            None
        } else {
            self.node(node_id).cache.get(input)
        }
    }

    fn cache_store(
        &mut self,
        node_id: taffy::NodeId,
        input: &LayoutInput,
        layout_output: LayoutOutput,
    ) {
        if self.first_error.is_none() {
            self.node_mut(node_id).cache.store(*input, layout_output);
        }
    }

    fn cache_clear(&mut self, node_id: taffy::NodeId) {
        self.node_mut(node_id).cache.clear();
    }
}

impl<M: TextMeasurer> LayoutPartialTree for DerivedLayoutTree<'_, M> {
    type CoreContainerStyle<'a>
        = &'a Style<String>
    where
        Self: 'a;
    type CustomIdent = String;

    fn get_core_container_style(&self, node_id: taffy::NodeId) -> Self::CoreContainerStyle<'_> {
        &self.node(node_id).style
    }

    fn resolve_calc_value(&self, _value: *const (), _basis: f32) -> f32 {
        0.0
    }

    fn set_unrounded_layout(&mut self, node_id: taffy::NodeId, layout: &Layout) {
        self.node_mut(node_id).unrounded = *layout;
    }

    fn compute_child_layout(
        &mut self,
        node_id: taffy::NodeId,
        inputs: LayoutInput,
    ) -> LayoutOutput {
        self.compute_node_layout(node_id, inputs, None)
    }
}

impl<M: TextMeasurer> LayoutBlockContainer for DerivedLayoutTree<'_, M> {
    type BlockContainerStyle<'a>
        = &'a Style<String>
    where
        Self: 'a;
    type BlockItemStyle<'a>
        = &'a Style<String>
    where
        Self: 'a;

    fn get_block_container_style(&self, node_id: taffy::NodeId) -> Self::BlockContainerStyle<'_> {
        &self.node(node_id).style
    }

    fn get_block_child_style(&self, child_node_id: taffy::NodeId) -> Self::BlockItemStyle<'_> {
        &self.node(child_node_id).style
    }

    fn compute_block_child_layout(
        &mut self,
        node_id: taffy::NodeId,
        inputs: LayoutInput,
        block_context: Option<&mut BlockContext<'_>>,
    ) -> LayoutOutput {
        self.compute_node_layout(node_id, inputs, block_context)
    }
}

impl<M: TextMeasurer> LayoutFlexboxContainer for DerivedLayoutTree<'_, M> {
    type FlexboxContainerStyle<'a>
        = &'a Style<String>
    where
        Self: 'a;
    type FlexboxItemStyle<'a>
        = &'a Style<String>
    where
        Self: 'a;

    fn get_flexbox_container_style(
        &self,
        node_id: taffy::NodeId,
    ) -> Self::FlexboxContainerStyle<'_> {
        &self.node(node_id).style
    }

    fn get_flexbox_child_style(&self, child_node_id: taffy::NodeId) -> Self::FlexboxItemStyle<'_> {
        &self.node(child_node_id).style
    }
}

impl<M: TextMeasurer> LayoutGridContainer for DerivedLayoutTree<'_, M> {
    type GridContainerStyle<'a>
        = &'a Style<String>
    where
        Self: 'a;
    type GridItemStyle<'a>
        = &'a Style<String>
    where
        Self: 'a;

    fn get_grid_container_style(&self, node_id: taffy::NodeId) -> Self::GridContainerStyle<'_> {
        &self.node(node_id).style
    }

    fn get_grid_child_style(&self, child_node_id: taffy::NodeId) -> Self::GridItemStyle<'_> {
        &self.node(child_node_id).style
    }
}

#[cfg(test)]
mod tests {
    use taffy::{
        geometry::{Line, Size},
        AvailableSpace, LayoutInput, RequestedAxis, SizingMode,
    };

    use crate::ui::{
        elements::{Dom, DomPublisher, Element, ElementTag},
        text::TextEngine,
    };

    use super::*;
    use crate::ui::layout::{reconcile::reconcile_full, LogicalViewport};

    #[derive(Debug)]
    struct RecordedRequest {
        width: AvailableSpace,
        final_paragraph_resolution: bool,
    }

    #[derive(Debug)]
    struct RecordingMeasurer {
        inner: TextEngine,
        calls: Vec<RecordedRequest>,
    }

    impl RecordingMeasurer {
        fn new() -> Self {
            Self {
                inner: TextEngine::without_system_fonts(),
                calls: Vec::new(),
            }
        }
    }

    impl TextMeasurer for RecordingMeasurer {
        fn measure(&mut self, request: TextMeasureRequest<'_>) -> Result<TextMeasurement, String> {
            self.calls.push(RecordedRequest {
                width: request.available_space().width,
                final_paragraph_resolution: request.is_final_paragraph_resolution(),
            });
            <TextEngine as TextMeasurer>::measure(&mut self.inner, request)
        }
    }

    fn probe_input(width: f32) -> LayoutInput {
        LayoutInput {
            run_mode: RunMode::ComputeSize,
            sizing_mode: SizingMode::InherentSize,
            axis: RequestedAxis::Both,
            known_dimensions: Size {
                width: Some(width),
                height: None,
            },
            parent_size: Size {
                width: Some(300.0),
                height: None,
            },
            available_space: Size {
                width: AvailableSpace::Definite(300.0),
                height: AvailableSpace::MaxContent,
            },
            vertical_margins_are_collapsible: Line::FALSE,
        }
    }

    #[test]
    fn post_layout_resolution_ignores_repeated_probe_cache_order() {
        let mut dom = Dom::new();
        let window = dom.create_element(Element::from_tag(ElementTag::Window));
        let paragraph = dom.create_element(Element::from_tag(ElementTag::Text));
        let text = dom.create_text("cached paragraph probe");
        dom.append_child(dom.root(), window).unwrap();
        dom.append_child(window, paragraph).unwrap();
        dom.append_child(paragraph, text).unwrap();
        let (_publisher, reader) = DomPublisher::new(&dom, |_| {});
        let publication = reader.load();
        let mut scratch = reconcile_full(
            publication.as_ref(),
            LogicalViewport::new(300.0, 200.0).unwrap(),
        )
        .unwrap();
        let paragraph_id = scratch.topology.layout_id(paragraph).unwrap();
        let narrow_input = probe_input(80.0);
        let wide_input = probe_input(160.0);
        let mut measurer = RecordingMeasurer::new();

        let final_paragraphs = {
            let mut tree = DerivedLayoutTree {
                topology: &scratch.topology,
                nodes: &mut scratch.nodes,
                measurer: &mut measurer,
                first_error: None,
            };
            let narrow_output =
                tree.compute_node_layout(paragraph_id.into_taffy(), narrow_input, None);
            tree.compute_node_layout(paragraph_id.into_taffy(), wide_input, None);
            let calls_after_distinct_probes = tree.measurer.calls.len();

            let cached_narrow_output =
                tree.compute_node_layout(paragraph_id.into_taffy(), narrow_input, None);
            assert_eq!(cached_narrow_output, narrow_output);
            assert_eq!(tree.measurer.calls.len(), calls_after_distinct_probes);
            assert!(tree
                .measurer
                .calls
                .iter()
                .all(|call| !call.final_paragraph_resolution));

            let mut completed_layout = Layout::new();
            completed_layout.size = narrow_output.size;
            tree.set_unrounded_layout(paragraph_id.into_taffy(), &completed_layout);
            tree.resolve_final_paragraphs().unwrap()
        };

        let final_calls = measurer
            .calls
            .iter()
            .filter(|call| call.final_paragraph_resolution)
            .collect::<Vec<_>>();
        assert_eq!(final_calls.len(), 1);
        assert_eq!(final_calls[0].width, AvailableSpace::Definite(80.0));
        assert_eq!(
            final_paragraphs
                .get(&paragraph)
                .unwrap()
                .constraint()
                .definite_value(),
            Some(80.0)
        );
    }
}
