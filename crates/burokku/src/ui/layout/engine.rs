use std::collections::HashSet;

use taffy::{geometry::Size, AvailableSpace};

use crate::ui::{
    elements::Dom,
    text::{TextConstraint, TextEngine},
};

use super::{
    computed::ComputedLayout,
    error::LayoutError,
    reconcile::reconcile_full,
    tree::{compute_layout, TextMeasureRequest, TextMeasurement, TextMeasurer},
    LogicalViewport,
};

/// UI-thread owner of the last complete computed layout and text measurement
/// implementation used by Taffy paragraph leaves.
#[derive(Debug)]
pub(crate) struct LayoutEngine<M> {
    measurer: M,
    current: Option<ComputedLayout>,
}

impl<M: TextMeasurer> LayoutEngine<M> {
    pub(crate) fn new(measurer: M) -> Self {
        Self {
            measurer,
            current: None,
        }
    }

    pub(crate) fn current(&self) -> Option<&ComputedLayout> {
        self.current.as_ref()
    }

    pub(crate) fn measurer_mut(&mut self) -> &mut M {
        &mut self.measurer
    }

    /// Reconcile and compute one live-DOM revision under a logical viewport.
    ///
    /// The previous complete state is replaced only after lowering, Taffy
    /// computation, measurement, and computed-box validation all succeed.
    pub(crate) fn compute(
        &mut self,
        dom: &Dom,
        viewport: LogicalViewport,
    ) -> Result<&ComputedLayout, LayoutError> {
        let text_generation = self.measurer.generation();
        if self.current.as_ref().is_some_and(|current| {
            current.revision() == dom.revision()
                && current.viewport() == viewport
                && current.text_generation() == text_generation
        }) {
            return Ok(self
                .current
                .as_ref()
                .expect("the matching current layout was checked above"));
        }

        let mut scratch = reconcile_full(dom, viewport)?;
        let final_paragraphs = compute_layout(&mut scratch, &mut self.measurer)?;
        let after_generation = self.measurer.generation();
        if after_generation != text_generation {
            return Err(LayoutError::TextGenerationChanged {
                before: text_generation,
                after: after_generation,
            });
        }
        let active_text_sources = final_paragraphs.keys().copied().collect::<HashSet<_>>();
        let next = ComputedLayout::from_scratch(scratch, text_generation, final_paragraphs)?;
        self.measurer.retain_sources(&active_text_sources);
        self.current = Some(next);
        Ok(self
            .current
            .as_ref()
            .expect("a successfully computed layout was just installed"))
    }
}
impl LayoutEngine<TextEngine> {
    pub(crate) fn remove_nodes(&mut self, nodes: &[crate::ui::elements::NodeId]) {
        self.measurer.remove_sources(nodes);
        if self
            .current
            .as_ref()
            .is_some_and(|current| nodes.iter().any(|node| current.box_for(*node).is_some()))
        {
            self.current = None;
        }
    }
}

impl TextMeasurer for TextEngine {
    fn generation(&self) -> u64 {
        TextEngine::generation(self)
    }

    fn measure(&mut self, request: TextMeasureRequest<'_>) -> Result<TextMeasurement, String> {
        let known = request.known_dimensions();
        let constraint = match request.available_space().width {
            AvailableSpace::MinContent => Ok(TextConstraint::MinContent),
            AvailableSpace::MaxContent => Ok(TextConstraint::MaxContent),
            AvailableSpace::Definite(width) => TextConstraint::definite(width),
        }
        .map_err(|error| error.to_string())?;
        let shaped = self
            .shape(request.paragraph(), constraint)
            .map_err(|error| error.to_string())?;
        let metrics = shaped.metrics();
        let size = Size {
            width: known.width.unwrap_or(metrics.width()),
            height: known.height.unwrap_or(metrics.height()),
        };
        if request.is_final_paragraph_resolution() {
            Ok(TextMeasurement::with_shaped(
                size,
                metrics.first_baseline(),
                shaped,
            ))
        } else {
            Ok(TextMeasurement::new(size, metrics.first_baseline()))
        }
    }

    fn retain_sources(&mut self, sources: &HashSet<crate::ui::elements::NodeId>) {
        TextEngine::retain_sources(self, sources);
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use taffy::{geometry::Size, AvailableSpace};

    use crate::ui::elements::{
        styles::grid::{GridStyle, GridTemplateComponent, TrackSizingFunction},
        Dom, Element, ElementTag,
    };

    use super::*;
    use crate::ui::layout::{TextMeasureRequest, TextMeasurement};

    #[derive(Clone, Debug)]
    struct RecordedMeasure {
        source: crate::ui::elements::NodeId,
        text: String,
        available_space: Size<AvailableSpace>,
        final_paragraph_resolution: bool,
    }

    #[derive(Debug)]
    struct TestMeasurer {
        calls: Vec<RecordedMeasure>,
        fail: bool,
        generation: u64,
        omit_final_paragraph: bool,
        final_width_delta: f32,
        text: TextEngine,
    }

    impl Default for TestMeasurer {
        fn default() -> Self {
            Self {
                calls: Vec::new(),
                fail: false,
                generation: 0,
                omit_final_paragraph: false,
                final_width_delta: 0.0,
                text: TextEngine::without_system_fonts(),
            }
        }
    }

    impl TextMeasurer for TestMeasurer {
        fn generation(&self) -> u64 {
            self.generation
        }

        fn measure(&mut self, request: TextMeasureRequest<'_>) -> Result<TextMeasurement, String> {
            if self.fail {
                return Err("injected measurement failure".into());
            }

            self.calls.push(RecordedMeasure {
                source: request.source(),
                text: request.text().to_owned(),
                available_space: request.available_space(),
                final_paragraph_resolution: request.is_final_paragraph_resolution(),
            });

            let intrinsic_width = request.text().chars().count() as f32 * 10.0;
            let available_width = match request.available_space().width {
                AvailableSpace::Definite(width) => Some(width),
                AvailableSpace::MinContent => Some(intrinsic_width.min(10.0)),
                AvailableSpace::MaxContent => None,
            };
            let measured_width = request.known_dimensions().width.unwrap_or_else(|| {
                available_width.map_or(intrinsic_width, |width| width.min(intrinsic_width))
            });
            let wrapping_width = available_width.unwrap_or(intrinsic_width).max(1.0);
            let lines = if intrinsic_width == 0.0 {
                1.0
            } else {
                (intrinsic_width / wrapping_width).ceil().max(1.0)
            };
            let measured_height = request.known_dimensions().height.unwrap_or(lines * 20.0);
            let baseline = if request.text().starts_with("low") {
                6.0
            } else {
                14.0
            };
            let size = Size {
                width: measured_width,
                height: measured_height,
            };
            if request.is_final_paragraph_resolution() {
                if self.omit_final_paragraph {
                    return Ok(TextMeasurement::new(size, Some(baseline)));
                }
                let constraint = match request.available_space().width {
                    AvailableSpace::MinContent => TextConstraint::MinContent,
                    AvailableSpace::MaxContent => TextConstraint::MaxContent,
                    AvailableSpace::Definite(width) => {
                        TextConstraint::definite(width + self.final_width_delta)
                            .map_err(|error| error.to_string())?
                    }
                };
                let shaped = self
                    .text
                    .shape(request.paragraph(), constraint)
                    .map_err(|error| error.to_string())?;
                Ok(TextMeasurement::with_shaped(size, Some(baseline), shaped))
            } else {
                Ok(TextMeasurement::new(size, Some(baseline)))
            }
        }
    }

    fn viewport(width: f32, height: f32) -> LogicalViewport {
        LogicalViewport::new(width, height).unwrap()
    }

    fn element(dom: &mut Dom, tag: ElementTag) -> crate::ui::elements::NodeId {
        dom.create_element(Element::from_tag(tag))
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.001,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn app_without_window_produces_a_successful_empty_layout() {
        let dom = Dom::new();
        let mut engine = LayoutEngine::new(TestMeasurer::default());

        let computed = engine.compute(&dom, viewport(800.0, 600.0)).unwrap();

        assert_eq!(computed.window(), None);
        assert!(computed.is_empty());
        assert_eq!(computed.revision(), dom.revision());
    }

    #[test]
    fn window_uses_the_actual_viewport_and_detached_nodes_are_omitted() {
        let mut dom = Dom::new();
        let window = element(&mut dom, ElementTag::Window);
        let child = element(&mut dom, ElementTag::Div);
        let detached = element(&mut dom, ElementTag::Grid);
        dom.set_style_property(child, "width", "10.5px").unwrap();
        dom.set_style_property(child, "height", "20px").unwrap();
        dom.append_child(dom.root(), window).unwrap();
        dom.append_child(window, child).unwrap();
        let mut engine = LayoutEngine::new(TestMeasurer::default());

        let computed = engine.compute(&dom, viewport(320.5, 240.25)).unwrap();

        let root = computed.box_for(window).unwrap().layout();
        assert_close(root.size.width, 320.5);
        assert_close(root.size.height, 240.25);
        assert_close(computed.box_for(child).unwrap().layout().size.width, 10.5);
        assert_eq!(
            computed.box_for(child).unwrap().layout_parent(),
            Some(window)
        );
        assert!(computed.box_for(detached).is_none());
        assert_eq!(computed.layout_children(window), Some(vec![child]));
        assert_eq!(computed.len(), 2);
    }

    #[test]
    fn flex_and_grid_dispatch_to_their_taffy_algorithms() {
        let mut dom = Dom::new();
        let window = element(&mut dom, ElementTag::Window);
        let flex = element(&mut dom, ElementTag::Flex);
        let first = element(&mut dom, ElementTag::Div);
        let second = element(&mut dom, ElementTag::Div);
        dom.set_style_property(flex, "width", "200px").unwrap();
        dom.set_style_property(flex, "height", "30px").unwrap();
        dom.set_style_property(first, "width", "40px").unwrap();
        dom.set_style_property(first, "height", "20px").unwrap();
        dom.set_style_property(second, "width", "60px").unwrap();
        dom.set_style_property(second, "height", "20px").unwrap();

        let grid = dom.create_element(Element::Grid {
            style: Box::new(GridStyle {
                template_columns: vec![
                    GridTemplateComponent::Single(TrackSizingFunction::length(50.0)),
                    GridTemplateComponent::Single(TrackSizingFunction::fraction(1.0)),
                ],
                ..GridStyle::default()
            }),
        });
        let grid_first = element(&mut dom, ElementTag::Div);
        let grid_second = element(&mut dom, ElementTag::Div);
        dom.set_style_property(grid, "width", "200px").unwrap();
        dom.set_style_property(grid, "height", "30px").unwrap();

        dom.append_child(dom.root(), window).unwrap();
        dom.append_child(window, flex).unwrap();
        dom.append_child(flex, first).unwrap();
        dom.append_child(flex, second).unwrap();
        dom.append_child(window, grid).unwrap();
        dom.append_child(grid, grid_first).unwrap();
        dom.append_child(grid, grid_second).unwrap();
        let mut engine = LayoutEngine::new(TestMeasurer::default());

        let computed = engine.compute(&dom, viewport(400.0, 300.0)).unwrap();

        assert_close(computed.box_for(first).unwrap().layout().location.x, 0.0);
        assert_close(computed.box_for(second).unwrap().layout().location.x, 40.0);
        assert_close(
            computed.box_for(grid_first).unwrap().layout().size.width,
            50.0,
        );
        assert_close(
            computed.box_for(grid_second).unwrap().layout().size.width,
            150.0,
        );
        assert_close(
            computed.box_for(grid_second).unwrap().layout().location.x,
            50.0,
        );
    }

    #[test]
    fn flex_stretch_resolves_the_completed_paragraph_width() {
        let mut dom = Dom::new();
        let window = element(&mut dom, ElementTag::Window);
        let flex = element(&mut dom, ElementTag::Flex);
        let paragraph = element(&mut dom, ElementTag::Text);
        let text = dom.create_text("stretch this paragraph across the flex container");
        dom.set_style_property(flex, "width", "180px").unwrap();
        dom.set_style_property(flex, "flex-direction", "column")
            .unwrap();
        dom.set_style_property(flex, "align-items", "stretch")
            .unwrap();
        dom.set_style_property(paragraph, "padding", "5px").unwrap();
        dom.append_child(dom.root(), window).unwrap();
        dom.append_child(window, flex).unwrap();
        dom.append_child(flex, paragraph).unwrap();
        dom.append_child(paragraph, text).unwrap();
        let mut engine = LayoutEngine::new(TestMeasurer::default());

        let computed = engine.compute(&dom, viewport(400.0, 300.0)).unwrap();
        let layout = computed.box_for(paragraph).unwrap().layout();
        let final_constraint = computed.final_paragraph(paragraph).unwrap().constraint();

        assert_close(layout.size.width, 180.0);
        assert_eq!(
            final_constraint,
            TextConstraint::definite(layout.content_box_width()).unwrap()
        );
        assert_eq!(
            engine
                .measurer
                .calls
                .iter()
                .filter(|call| call.source == paragraph && call.final_paragraph_resolution)
                .count(),
            1
        );
    }

    #[test]
    fn grid_stretch_resolves_the_completed_paragraph_width() {
        let mut dom = Dom::new();
        let window = element(&mut dom, ElementTag::Window);
        let grid = dom.create_element(Element::Grid {
            style: Box::new(GridStyle {
                template_columns: vec![GridTemplateComponent::Single(
                    TrackSizingFunction::fraction(1.0),
                )],
                ..GridStyle::default()
            }),
        });
        let paragraph = element(&mut dom, ElementTag::Text);
        let text = dom.create_text("stretch this paragraph across the grid track");
        dom.set_style_property(grid, "width", "210px").unwrap();
        dom.set_style_property(grid, "justify-items", "stretch")
            .unwrap();
        dom.set_style_property(paragraph, "padding", "7px").unwrap();
        dom.append_child(dom.root(), window).unwrap();
        dom.append_child(window, grid).unwrap();
        dom.append_child(grid, paragraph).unwrap();
        dom.append_child(paragraph, text).unwrap();
        let mut engine = LayoutEngine::new(TestMeasurer::default());

        let computed = engine.compute(&dom, viewport(400.0, 300.0)).unwrap();
        let layout = computed.box_for(paragraph).unwrap().layout();
        let final_constraint = computed.final_paragraph(paragraph).unwrap().constraint();

        assert_close(layout.size.width, 210.0);
        assert_eq!(
            final_constraint,
            TextConstraint::definite(layout.content_box_width()).unwrap()
        );
        assert_eq!(
            engine
                .measurer
                .calls
                .iter()
                .filter(|call| call.source == paragraph && call.final_paragraph_resolution)
                .count(),
            1
        );
    }

    #[test]
    fn visible_paragraph_requires_a_final_shaped_result() {
        let mut dom = Dom::new();
        let window = element(&mut dom, ElementTag::Window);
        let paragraph = element(&mut dom, ElementTag::Text);
        let text = dom.create_text("required final paragraph");
        dom.append_child(dom.root(), window).unwrap();
        dom.append_child(window, paragraph).unwrap();
        dom.append_child(paragraph, text).unwrap();
        let measurer = TestMeasurer {
            omit_final_paragraph: true,
            ..TestMeasurer::default()
        };
        let mut engine = LayoutEngine::new(measurer);

        let error = engine.compute(&dom, viewport(300.0, 200.0)).unwrap_err();

        assert!(matches!(
            error,
            LayoutError::InvalidFinalParagraph(source) if source == paragraph
        ));
    }

    #[test]
    fn final_paragraph_must_match_the_completed_content_width() {
        let mut dom = Dom::new();
        let window = element(&mut dom, ElementTag::Window);
        let paragraph = element(&mut dom, ElementTag::Text);
        let text = dom.create_text("wrong width must fail");
        dom.set_style_property(paragraph, "width", "100px").unwrap();
        dom.append_child(dom.root(), window).unwrap();
        dom.append_child(window, paragraph).unwrap();
        dom.append_child(paragraph, text).unwrap();
        let measurer = TestMeasurer {
            final_width_delta: 1.0,
            ..TestMeasurer::default()
        };
        let mut engine = LayoutEngine::new(measurer);

        let error = engine.compute(&dom, viewport(300.0, 200.0)).unwrap_err();

        assert!(matches!(
            error,
            LayoutError::InvalidFinalParagraph(source) if source == paragraph
        ));
    }

    #[test]
    fn paragraph_descendants_flatten_into_one_measured_leaf() {
        let mut dom = Dom::new();
        let window = element(&mut dom, ElementTag::Window);
        let paragraph = element(&mut dom, ElementTag::Text);
        let first_text = dom.create_text("hello ");
        let nested = element(&mut dom, ElementTag::Text);
        let second_text = dom.create_text("world");
        dom.set_style_property(paragraph, "width", "100px").unwrap();
        dom.set_style_property(paragraph, "padding", "5px").unwrap();
        dom.append_child(dom.root(), window).unwrap();
        dom.append_child(window, paragraph).unwrap();
        dom.append_child(paragraph, first_text).unwrap();
        dom.append_child(paragraph, nested).unwrap();
        dom.append_child(nested, second_text).unwrap();
        let mut engine = LayoutEngine::new(TestMeasurer::default());

        let computed = engine.compute(&dom, viewport(300.0, 200.0)).unwrap();

        assert_eq!(computed.len(), 2);
        assert_eq!(computed.layout_children(paragraph), Some(Vec::new()));
        assert_eq!(computed.text_owner(first_text), Some(paragraph));
        assert_eq!(computed.text_owner(nested), Some(paragraph));
        assert_eq!(computed.text_owner(second_text), Some(paragraph));
        assert_close(
            computed.box_for(paragraph).unwrap().layout().size.width,
            100.0,
        );
        assert!(engine
            .measurer
            .calls
            .iter()
            .all(|call| call.source == paragraph && call.text == "hello world"));
        let final_calls = engine
            .measurer
            .calls
            .iter()
            .filter(|call| call.final_paragraph_resolution)
            .collect::<Vec<_>>();
        assert_eq!(
            final_calls.len(),
            1,
            "paragraph resolution runs once after Taffy finishes"
        );
        assert_eq!(
            final_calls[0].available_space.width,
            AvailableSpace::Definite(90.0)
        );
    }

    #[test]
    fn paragraph_baselines_align_in_a_flex_container() {
        let mut dom = Dom::new();
        let window = element(&mut dom, ElementTag::Window);
        let flex = element(&mut dom, ElementTag::Flex);
        let low = element(&mut dom, ElementTag::Text);
        let high = element(&mut dom, ElementTag::Text);
        let low_text = dom.create_text("low");
        let high_text = dom.create_text("high");
        dom.set_style_property(flex, "align-items", "baseline")
            .unwrap();
        dom.append_child(dom.root(), window).unwrap();
        dom.append_child(window, flex).unwrap();
        dom.append_child(flex, low).unwrap();
        dom.append_child(flex, high).unwrap();
        dom.append_child(low, low_text).unwrap();
        dom.append_child(high, high_text).unwrap();
        let mut engine = LayoutEngine::new(TestMeasurer::default());

        let computed = engine.compute(&dom, viewport(300.0, 200.0)).unwrap();

        let low_baseline = computed.box_for(low).unwrap().content_origin().y + 6.0;
        let high_baseline = computed.box_for(high).unwrap().content_origin().y + 14.0;
        assert_close(low_baseline, high_baseline);
    }

    #[test]
    fn full_rebuild_tracks_reparenting_and_child_order_without_changing_ids() {
        let mut staging = Dom::new();
        let window = element(&mut staging, ElementTag::Window);
        let first_parent = element(&mut staging, ElementTag::Div);
        let second_parent = element(&mut staging, ElementTag::Div);
        let child = element(&mut staging, ElementTag::Div);
        staging.append_child(staging.root(), window).unwrap();
        staging.append_child(window, first_parent).unwrap();
        staging.append_child(window, second_parent).unwrap();
        staging.append_child(first_parent, child).unwrap();
        let mut engine = LayoutEngine::new(TestMeasurer::default());

        engine.compute(&staging, viewport(300.0, 200.0)).unwrap();
        assert_eq!(
            engine
                .current()
                .unwrap()
                .box_for(child)
                .unwrap()
                .layout_parent(),
            Some(first_parent)
        );

        staging.append_child(second_parent, child).unwrap();
        staging.insert_child(window, 0, second_parent).unwrap();
        let computed = engine.compute(&staging, viewport(300.0, 200.0)).unwrap();

        assert_eq!(
            computed.box_for(child).unwrap().layout_parent(),
            Some(second_parent)
        );
        assert_eq!(
            computed.layout_children(window),
            Some(vec![second_parent, first_parent])
        );
        assert_eq!(computed.layout_children(second_parent), Some(vec![child]));
    }

    #[test]
    fn measurement_failure_keeps_the_previous_complete_revision() {
        let mut staging = Dom::new();
        let window = element(&mut staging, ElementTag::Window);
        let div = element(&mut staging, ElementTag::Div);
        staging.append_child(staging.root(), window).unwrap();
        staging.append_child(window, div).unwrap();
        let mut engine = LayoutEngine::new(TestMeasurer::default());
        engine.compute(&staging, viewport(300.0, 200.0)).unwrap();
        let old_revision = engine.current().unwrap().revision();
        let paragraph = element(&mut staging, ElementTag::Text);
        let text = staging.create_text("fails");
        staging.append_child(window, paragraph).unwrap();
        staging.append_child(paragraph, text).unwrap();
        engine.measurer_mut().fail = true;

        let error = engine
            .compute(&staging, viewport(300.0, 200.0))
            .unwrap_err();

        assert!(matches!(error, LayoutError::TextMeasurement { .. }));
        let current = engine.current().unwrap();
        assert_eq!(current.revision(), old_revision);
        assert!(current.box_for(paragraph).is_none());
    }

    #[test]
    fn dom_mutation_and_viewport_changes_recompute_the_root() {
        let mut dom = Dom::new();
        let window = element(&mut dom, ElementTag::Window);
        dom.append_child(dom.root(), window).unwrap();
        let mut engine = LayoutEngine::new(TestMeasurer::default());
        engine.compute(&dom, viewport(100.0, 80.0)).unwrap();
        let first_revision = engine.current().unwrap().revision();

        dom.set_attribute(window, "title".into(), "new".into())
            .unwrap();
        let computed = engine.compute(&dom, viewport(200.0, 150.0)).unwrap();
        assert!(computed.revision() > first_revision);
        assert_close(computed.box_for(window).unwrap().layout().size.width, 200.0);
    }

    #[test]
    fn identical_revision_and_viewport_reuse_the_computed_frame() {
        let mut dom = Dom::new();
        let window = element(&mut dom, ElementTag::Window);
        let paragraph = element(&mut dom, ElementTag::Text);
        let text = dom.create_text("cached");
        dom.append_child(dom.root(), window).unwrap();
        dom.append_child(window, paragraph).unwrap();
        dom.append_child(paragraph, text).unwrap();
        let mut engine = LayoutEngine::new(TestMeasurer::default());

        engine.compute(&dom, viewport(300.0, 200.0)).unwrap();
        let call_count = engine.measurer.calls.len();
        engine.compute(&dom, viewport(300.0, 200.0)).unwrap();
        assert_eq!(engine.measurer.calls.len(), call_count);

        engine.measurer_mut().generation += 1;
        engine.compute(&dom, viewport(300.0, 200.0)).unwrap();
        assert!(engine.measurer.calls.len() > call_count);
        assert_eq!(engine.current().unwrap().text_generation(), 1);
    }

    #[test]
    fn block_text_percentage_padding_resolves_all_sides_against_parent_width() {
        // <Window viewport="300px 200px">
        //   <Div style="width: 100px; height: auto">
        //     <Text style="padding: 10%">hello</Text>
        //   </Div>
        // </Window>
        let mut dom = Dom::new();
        let window = element(&mut dom, ElementTag::Window);
        let block = element(&mut dom, ElementTag::Div);
        let paragraph = element(&mut dom, ElementTag::Text);
        let text = dom.create_text("hello");
        dom.set_style_property(block, "width", "100px").unwrap();
        dom.set_style_property(paragraph, "padding", "10%").unwrap();
        dom.append_child(dom.root(), window).unwrap();
        dom.append_child(window, block).unwrap();
        dom.append_child(block, paragraph).unwrap();
        dom.append_child(paragraph, text).unwrap();
        let mut engine = LayoutEngine::new(TestMeasurer::default());

        let computed = engine.compute(&dom, viewport(300.0, 200.0)).unwrap();
        let paragraph_box = computed.box_for(paragraph).unwrap();
        let layout = paragraph_box.layout();

        assert_close(layout.size.width, 100.0);
        assert_close(layout.padding.left, 10.0);
        assert_close(layout.padding.right, 10.0);
        assert_close(layout.size.height, 40.0);
        assert_close(layout.padding.top, 10.0);
        assert_close(layout.padding.bottom, 10.0);
        assert_close(computed.box_for(block).unwrap().layout().size.height, 40.0);
        assert_close(
            paragraph_box.content_origin().y - paragraph_box.border_origin().y,
            10.0,
        );
    }

    #[test]
    fn nested_block_vertical_percentage_padding_uses_outer_width() {
        // <Div class="outer" style="width: 600px; height: 200px; background: yellow">
        //   <Div class="inner" style="padding: 50% 20%; background: red">
        //     <Text class="content" style="background: blue">hi</Text>
        //   </Div>
        // </Div>
        let mut dom = Dom::new();
        let window = element(&mut dom, ElementTag::Window);
        let outer = element(&mut dom, ElementTag::Div);
        let inner = element(&mut dom, ElementTag::Div);
        let content = element(&mut dom, ElementTag::Text);
        let text = dom.create_text("hi");
        dom.set_style_property(outer, "width", "600px").unwrap();
        dom.set_style_property(outer, "height", "200px").unwrap();
        dom.set_style_property(outer, "background-color", "#ffff00")
            .unwrap();
        dom.set_style_property(inner, "padding", "50% 20%").unwrap();
        dom.set_style_property(inner, "background-color", "#ff0000")
            .unwrap();
        dom.set_style_property(content, "background-color", "#0000ff")
            .unwrap();
        dom.append_child(dom.root(), window).unwrap();
        dom.append_child(window, outer).unwrap();
        dom.append_child(outer, inner).unwrap();
        dom.append_child(inner, content).unwrap();
        dom.append_child(content, text).unwrap();
        let mut engine = LayoutEngine::new(TestMeasurer::default());

        let computed = engine.compute(&dom, viewport(800.0, 700.0)).unwrap();
        let inner_box = computed.box_for(inner).unwrap();
        let layout = inner_box.layout();

        assert_close(layout.padding.top, 300.0);
        assert_close(layout.padding.bottom, 300.0);
        assert_close(layout.padding.left, 120.0);
        assert_close(layout.padding.right, 120.0);
        assert_close(layout.size.height, 620.0);
        assert_close(
            inner_box.content_origin().y - inner_box.border_origin().y,
            300.0,
        );
    }

    #[test]
    fn grid_item_percentage_padding_keeps_grid_area_as_its_basis() {
        // <Window viewport="300px 200px">
        //   <Grid style="width: 200px; grid-template-columns: 50px 1fr">
        //     <Text style="padding: 10%">hello</Text>
        //     <Div />
        //   </Grid>
        // </Window>
        let mut dom = Dom::new();
        let window = element(&mut dom, ElementTag::Window);
        let grid = dom.create_element(Element::Grid {
            style: Box::new(GridStyle {
                template_columns: vec![
                    GridTemplateComponent::Single(TrackSizingFunction::length(50.0)),
                    GridTemplateComponent::Single(TrackSizingFunction::fraction(1.0)),
                ],
                ..GridStyle::default()
            }),
        });
        let paragraph = element(&mut dom, ElementTag::Text);
        let text = dom.create_text("hello");
        let filler = element(&mut dom, ElementTag::Div);
        dom.set_style_property(grid, "width", "200px").unwrap();
        dom.set_style_property(paragraph, "padding", "10%").unwrap();
        dom.append_child(dom.root(), window).unwrap();
        dom.append_child(window, grid).unwrap();
        dom.append_child(grid, paragraph).unwrap();
        dom.append_child(paragraph, text).unwrap();
        dom.append_child(grid, filler).unwrap();
        let mut engine = LayoutEngine::new(TestMeasurer::default());

        let computed = engine.compute(&dom, viewport(300.0, 200.0)).unwrap();
        let layout = computed.box_for(paragraph).unwrap().layout();

        assert_close(layout.size.width, 50.0);
        assert_close(layout.padding.left, 5.0);
        assert_close(layout.padding.right, 5.0);
        assert_close(layout.padding.top, 5.0);
        assert_close(layout.padding.bottom, 5.0);
    }

    #[test]
    fn absolute_content_origins_include_parent_padding_once() {
        let mut dom = Dom::new();
        let window = element(&mut dom, ElementTag::Window);
        let parent = element(&mut dom, ElementTag::Div);
        let child = element(&mut dom, ElementTag::Div);
        dom.set_style_property(parent, "padding", "7.5px").unwrap();
        dom.set_style_property(child, "width", "20px").unwrap();
        dom.set_style_property(child, "height", "10px").unwrap();
        dom.append_child(dom.root(), window).unwrap();
        dom.append_child(window, parent).unwrap();
        dom.append_child(parent, child).unwrap();
        let mut engine = LayoutEngine::new(TestMeasurer::default());

        let computed = engine.compute(&dom, viewport(300.0, 200.0)).unwrap();
        let parent_box = computed.box_for(parent).unwrap();
        let child_box = computed.box_for(child).unwrap();

        assert_close(
            parent_box.content_origin().x - parent_box.border_origin().x,
            7.5,
        );
        assert_close(
            parent_box.content_origin().y - parent_box.border_origin().y,
            7.5,
        );
        assert_close(child_box.border_origin().x, parent_box.content_origin().x);
        assert_close(child_box.border_origin().y, parent_box.content_origin().y);
    }

    #[test]
    fn invalid_viewports_are_rejected_before_layout() {
        assert!(matches!(
            LogicalViewport::new(f32::NAN, 10.0),
            Err(LayoutError::InvalidViewport { .. })
        ));
        assert!(matches!(
            LogicalViewport::new(10.0, -1.0),
            Err(LayoutError::InvalidViewport { .. })
        ));
    }

    #[test]
    fn parley_measurement_wraps_at_the_content_box_width() {
        const TEST_FONT: &[u8] = include_bytes!("../../../testdata/fonts/NotoSans-Regular.ttf");

        let mut dom = Dom::new();
        let window = element(&mut dom, ElementTag::Window);
        let flex = element(&mut dom, ElementTag::Flex);
        let paragraph = element(&mut dom, ElementTag::Text);
        let text = dom.create_text("a bb cc a bb cc a bb cc a bb cc a bb cc");
        dom.set_style_property(flex, "width", "60px").unwrap();
        dom.set_style_property(paragraph, "font-family", "Noto Sans")
            .unwrap();
        dom.set_style_property(paragraph, "padding", "5px").unwrap();
        dom.append_child(dom.root(), window).unwrap();
        dom.append_child(window, flex).unwrap();
        dom.append_child(flex, paragraph).unwrap();
        dom.append_child(paragraph, text).unwrap();
        let mut text = TextEngine::without_system_fonts();
        text.register_font_data(TEST_FONT.to_vec()).unwrap();
        let mut engine = LayoutEngine::new(text);

        let computed = engine.compute(&dom, viewport(300.0, 300.0)).unwrap();
        let paragraph_box = computed.box_for(paragraph).unwrap();
        let final_paragraph = computed.final_paragraph(paragraph).unwrap();

        assert_eq!(final_paragraph.constraint().definite_value(), Some(50.0));
        assert_close(
            paragraph_box.layout().size.height,
            final_paragraph.metrics().height() + 10.0,
        );
    }

    #[test]
    fn parley_engine_retains_the_exact_final_width_paragraph() {
        const TEST_FONT: &[u8] = include_bytes!("../../../testdata/fonts/NotoSans-Regular.ttf");

        let mut staging = Dom::new();
        let window = element(&mut staging, ElementTag::Window);
        let paragraph = element(&mut staging, ElementTag::Text);
        let first = staging.create_text("hello ");
        let nested = element(&mut staging, ElementTag::Text);
        let second = staging.create_text("styled text");
        staging
            .set_style_property(paragraph, "font-family", "Noto Sans")
            .unwrap();
        staging
            .set_style_property(paragraph, "width", "100px")
            .unwrap();
        staging
            .set_style_property(paragraph, "padding", "5px")
            .unwrap();
        staging
            .set_style_property(nested, "color", "#ff0000")
            .unwrap();
        staging.append_child(staging.root(), window).unwrap();
        staging.append_child(window, paragraph).unwrap();
        staging.append_child(paragraph, first).unwrap();
        staging.append_child(paragraph, nested).unwrap();
        staging.append_child(nested, second).unwrap();
        let mut text = TextEngine::without_system_fonts();
        text.register_font_data(TEST_FONT.to_vec()).unwrap();
        let mut engine = LayoutEngine::new(text);

        let computed = engine.compute(&staging, viewport(300.0, 200.0)).unwrap();
        let first_paragraph = Rc::clone(computed.final_paragraph(paragraph).unwrap());

        assert_eq!(first_paragraph.constraint().definite_value(), Some(90.0));
        assert_eq!(first_paragraph.source(), paragraph);
        assert_eq!(first_paragraph.layout().scale(), 1.0);
        assert!(first_paragraph.layout().lines().any(|line| line
            .items()
            .any(|item| matches!(item, parley::PositionedLayoutItem::GlyphRun(run) if run.style().brush == [255, 0, 0, 255]))));
        let glyph_batches = crate::ui::text::paint::prepare_glyph_batches(
            computed.box_for(paragraph).unwrap().content_origin(),
            &first_paragraph,
        )
        .unwrap();
        assert!(
            glyph_batches
                .iter()
                .map(|batch| batch.glyphs().len())
                .sum::<usize>()
                > 0
        );

        staging
            .set_style_property(nested, "font-size", "28px")
            .unwrap();
        let computed = engine.compute(&staging, viewport(300.0, 200.0)).unwrap();
        let second_paragraph = computed.final_paragraph(paragraph).unwrap();

        assert_ne!(
            first_paragraph.fingerprint(),
            second_paragraph.fingerprint()
        );
        assert!(!Rc::ptr_eq(&first_paragraph, second_paragraph));
        assert_eq!(computed.revision(), staging.revision());
    }
}
