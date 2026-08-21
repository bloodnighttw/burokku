use std::rc::Rc;

use taffy::util::ResolveOrZero;
use taffy::{
    compute_block_layout, compute_cached_layout, compute_flexbox_layout, compute_grid_layout,
    compute_hidden_layout, compute_leaf_layout,
    geometry::{Point, Size},
    AvailableSpace, BlockContext, CacheTree, Display, Layout, LayoutBlockContainer,
    LayoutFlexboxContainer, LayoutGridContainer, LayoutInput, LayoutOutput, LayoutPartialTree,
    Overflow, RunMode, Style, TraversePartialTree, TraverseTree,
};

use crate::ui::elements::NodeId as DomNodeId;

use super::{
    error::LayoutError,
    reconcile::{LayoutNodeState, LayoutRole, ParagraphInput, ScratchLayout},
    topology::{LayoutId, LayoutTopology},
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct TextMeasureRequest<'a> {
    source: DomNodeId,
    text: &'a str,
    known_dimensions: Size<Option<f32>>,
    available_space: Size<AvailableSpace>,
    final_width_selection: bool,
}

impl<'a> TextMeasureRequest<'a> {
    pub(crate) fn source(self) -> DomNodeId {
        self.source
    }

    pub(crate) fn text(self) -> &'a str {
        self.text
    }

    pub(crate) fn known_dimensions(self) -> Size<Option<f32>> {
        self.known_dimensions
    }

    pub(crate) fn available_space(self) -> Size<AvailableSpace> {
        self.available_space
    }

    pub(crate) fn is_final_width_selection(self) -> bool {
        self.final_width_selection
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextMeasurement {
    size: Size<f32>,
    first_baseline: Option<f32>,
}

impl TextMeasurement {
    pub(crate) fn new(size: Size<f32>, first_baseline: Option<f32>) -> Self {
        Self {
            size,
            first_baseline,
        }
    }

    pub(crate) fn size(self) -> Size<f32> {
        self.size
    }

    pub(crate) fn first_baseline(self) -> Option<f32> {
        self.first_baseline
    }
}

pub(crate) trait TextMeasurer {
    /// Generation of external measurement inputs such as fonts or shaping
    /// configuration. Changing it invalidates an otherwise unchanged frame.
    fn generation(&self) -> u64 {
        0
    }

    fn measure(&mut self, request: TextMeasureRequest<'_>) -> Result<TextMeasurement, String>;
}

pub(super) fn compute_layout<M: TextMeasurer>(
    scratch: &mut ScratchLayout,
    measurer: &mut M,
) -> Result<(), LayoutError> {
    let Some(root) = scratch.topology.root() else {
        return Ok(());
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
    match tree.first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
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
                    .map_or(Size::ZERO, TextMeasurement::size)
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
        let final_measurement = self.measure_paragraph(
            &paragraph,
            Size {
                width: Some(content_width),
                height: None,
            },
            Size {
                width: AvailableSpace::Definite(content_width),
                height: input.available_space.height,
            },
            true,
        );
        if let Some(measurement) = final_measurement {
            output.first_baselines = Point {
                x: None,
                y: measurement
                    .first_baseline()
                    .map(|baseline| border.top + padding.top + baseline),
            };
        }
        output
    }

    fn measure_paragraph(
        &mut self,
        paragraph: &ParagraphInput,
        known_dimensions: Size<Option<f32>>,
        available_space: Size<AvailableSpace>,
        final_width_selection: bool,
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
            source: paragraph.source(),
            text: paragraph.text(),
            known_dimensions,
            available_space,
            final_width_selection,
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
