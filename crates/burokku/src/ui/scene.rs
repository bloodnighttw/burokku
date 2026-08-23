//! Revision-tagged scene planning and Vello construction.

use thiserror::Error;
use vello_common::{
    kurbo::{Affine, Rect},
    paint::Color,
};
use vello_hybrid::{Resources, Scene};
use winit::PhysicalSize;

use super::{
    elements::{styles::color::RgbaColor, NodeId},
    layout::{ComputedLayout, LogicalViewport},
    text::{paint::paint_paragraph, TextError},
};

pub(crate) const MAX_VELLO_SCENE_DIMENSION: u32 = u16::MAX as u32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LogicalRect {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

impl LogicalRect {
    fn contains(self, x: f32, y: f32) -> bool {
        self.width > 0.0
            && self.height > 0.0
            && x >= self.x
            && y >= self.y
            && x < self.x + self.width
            && y < self.y + self.height
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PaintItem {
    Background {
        node: NodeId,
        rect: LogicalRect,
        color: RgbaColor,
    },
    Text {
        source: NodeId,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HitRegion {
    node: NodeId,
    rect: LogicalRect,
}

#[derive(Clone, Debug)]
pub(crate) struct ScenePlan {
    revision: u64,
    viewport: LogicalViewport,
    physical_size: PhysicalSize<u32>,
    scale_factor: f64,
    items: Vec<PaintItem>,
    hit_regions: Vec<HitRegion>,
}

impl ScenePlan {
    pub(crate) fn from_layout(
        computed: &ComputedLayout,
        physical_size: PhysicalSize<u32>,
        scale_factor: f64,
    ) -> Result<Self, SceneError> {
        validate_target(physical_size, scale_factor)?;

        let mut items = Vec::new();
        let mut hit_regions = Vec::new();
        let snapshot = computed.publication().snapshot();
        for (node, _) in snapshot.iter() {
            let Some(computed_box) = computed.box_for(node) else {
                continue;
            };
            let layout = computed_box.layout();
            let origin = computed_box.border_origin();
            let rect = LogicalRect {
                x: origin.x,
                y: origin.y,
                width: layout.size.width,
                height: layout.size.height,
            };
            validate_rect(node, rect)?;
            hit_regions.push(HitRegion { node, rect });

            if let Some(color) = snapshot
                .element(node)
                .and_then(|element| element.background_color())
            {
                items.push(PaintItem::Background { node, rect, color });
            }
            if computed.final_paragraph(node).is_some() {
                items.push(PaintItem::Text { source: node });
            }
        }

        Ok(Self {
            revision: computed.revision(),
            viewport: computed.viewport(),
            physical_size,
            scale_factor,
            items,
            hit_regions,
        })
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn viewport(&self) -> LogicalViewport {
        self.viewport
    }

    pub(crate) fn physical_size(&self) -> PhysicalSize<u32> {
        self.physical_size
    }

    pub(crate) fn scale_factor(&self) -> f64 {
        self.scale_factor
    }

    pub(crate) fn items(&self) -> &[PaintItem] {
        &self.items
    }

    /// Hit test in logical viewport coordinates, walking reverse paint/source
    /// order so the deepest later-painted box wins.
    pub(crate) fn hit_test(&self, x: f32, y: f32) -> Option<NodeId> {
        let viewport = self.viewport();
        if !x.is_finite()
            || !y.is_finite()
            || x < 0.0
            || y < 0.0
            || x >= viewport.width()
            || y >= viewport.height()
        {
            return None;
        }
        self.hit_regions
            .iter()
            .rev()
            .find(|region| region.rect.contains(x, y))
            .map(|region| region.node)
    }

    pub(crate) fn hit_test_physical(&self, x: f64, y: f64) -> Option<NodeId> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let logical_x = x / self.scale_factor();
        let logical_y = y / self.scale_factor();
        if logical_x < f64::from(f32::MIN)
            || logical_x > f64::from(f32::MAX)
            || logical_y < f64::from(f32::MIN)
            || logical_y > f64::from(f32::MAX)
        {
            return None;
        }
        self.hit_test(logical_x as f32, logical_y as f32)
    }
}

#[derive(Debug)]
pub(crate) struct BuiltScene {
    scene: Scene,
    plan: ScenePlan,
    glyph_runs: usize,
    glyphs: usize,
}

impl BuiltScene {
    pub(crate) fn build(
        computed: &ComputedLayout,
        physical_size: PhysicalSize<u32>,
        scale_factor: f64,
        resources: &mut Resources,
    ) -> Result<Self, SceneError> {
        let plan = ScenePlan::from_layout(computed, physical_size, scale_factor)?;
        let width = u16::try_from(physical_size.width)
            .expect("scene target width was validated before Vello construction");
        let height = u16::try_from(physical_size.height)
            .expect("scene target height was validated before Vello construction");
        let mut scene = Scene::new(width, height);
        scene.set_transform(Affine::scale(scale_factor));
        let mut glyph_runs = 0;
        let mut glyphs = 0;

        for item in plan.items() {
            match *item {
                PaintItem::Background { rect, color, .. } => {
                    scene.set_paint(Color::from_rgba8(
                        color.red,
                        color.green,
                        color.blue,
                        color.alpha,
                    ));
                    scene.fill_rect(&Rect::new(
                        f64::from(rect.x),
                        f64::from(rect.y),
                        f64::from(rect.x + rect.width),
                        f64::from(rect.y + rect.height),
                    ));
                }
                PaintItem::Text { source } => {
                    let paragraph = computed
                        .final_paragraph(source)
                        .ok_or(SceneError::MissingFinalParagraph(source))?;
                    let origin = computed
                        .box_for(source)
                        .ok_or(SceneError::MissingComputedBox(source))?
                        .content_origin();
                    let stats = paint_paragraph(&mut scene, resources, origin, paragraph)?;
                    glyph_runs += stats.runs();
                    glyphs += stats.glyphs();
                }
            }
        }

        Ok(Self {
            scene,
            plan,
            glyph_runs,
            glyphs,
        })
    }

    pub(crate) fn scene(&self) -> &Scene {
        &self.scene
    }

    pub(crate) fn plan(&self) -> &ScenePlan {
        &self.plan
    }

    pub(crate) fn glyph_runs(&self) -> usize {
        self.glyph_runs
    }

    pub(crate) fn glyphs(&self) -> usize {
        self.glyphs
    }
}

fn validate_target(size: PhysicalSize<u32>, scale_factor: f64) -> Result<(), SceneError> {
    if size.width == 0 || size.height == 0 {
        return Err(SceneError::EmptyTarget);
    }
    if size.width > MAX_VELLO_SCENE_DIMENSION || size.height > MAX_VELLO_SCENE_DIMENSION {
        return Err(SceneError::TargetTooLarge {
            size,
            max_dimension: MAX_VELLO_SCENE_DIMENSION,
        });
    }
    if !scale_factor.is_finite() || scale_factor <= 0.0 {
        return Err(SceneError::InvalidScaleFactor(scale_factor));
    }
    Ok(())
}

fn validate_rect(node: NodeId, rect: LogicalRect) -> Result<(), SceneError> {
    for (field, value) in [
        ("x", rect.x),
        ("y", rect.y),
        ("width", rect.width),
        ("height", rect.height),
    ] {
        if !value.is_finite() || matches!(field, "width" | "height") && value < 0.0 {
            return Err(SceneError::InvalidGeometry { node, field, value });
        }
    }
    Ok(())
}

#[derive(Debug, Error)]
pub(crate) enum SceneError {
    #[error("cannot build a scene for a zero-sized target")]
    EmptyTarget,

    #[error("display scale factor must be positive and finite, got {0}")]
    InvalidScaleFactor(f64),

    #[error("physical target {size:?} exceeds Vello's {max_dimension}-pixel dimension limit")]
    TargetTooLarge {
        size: PhysicalSize<u32>,
        max_dimension: u32,
    },

    #[error("node {node:?} has invalid scene {field} value {value}")]
    InvalidGeometry {
        node: NodeId,
        field: &'static str,
        value: f32,
    },

    #[error("paragraph {0:?} has no final shaped paragraph")]
    MissingFinalParagraph(NodeId),

    #[error("node {0:?} has no computed layout box")]
    MissingComputedBox(NodeId),

    #[error(transparent)]
    Text(#[from] TextError),
}

#[cfg(test)]
mod tests {
    use crate::ui::{
        elements::{Dom, DomPublisher, Element, ElementTag},
        layout::{LayoutEngine, LogicalViewport},
        text::TextEngine,
    };

    use super::*;

    const TEST_FONT: &[u8] = include_bytes!("../../testdata/fonts/NotoSans-Regular.ttf");

    fn computed_fixture() -> (LayoutEngine<TextEngine>, NodeId, NodeId, NodeId) {
        let mut dom = Dom::new();
        let window = dom.create_element(Element::from_tag(ElementTag::Window));
        let div = dom.create_element(Element::from_tag(ElementTag::Div));
        let text = dom.create_element(Element::from_tag(ElementTag::Text));
        let raw = dom.create_text("paint me");
        dom.set_style_property(window, "background-color", "#010203ff")
            .unwrap();
        dom.set_style_property(div, "width", "100px").unwrap();
        dom.set_style_property(div, "height", "80px").unwrap();
        dom.set_style_property(div, "background-color", "#112233ff")
            .unwrap();
        dom.set_style_property(text, "font-family", "Noto Sans")
            .unwrap();
        dom.append_child(dom.root(), window).unwrap();
        dom.append_child(window, div).unwrap();
        dom.append_child(div, text).unwrap();
        dom.append_child(text, raw).unwrap();
        let (_publisher, reader) = DomPublisher::new(&dom, |_| {});
        let mut text_engine = TextEngine::without_system_fonts();
        text_engine.register_font_data(TEST_FONT.to_vec()).unwrap();
        let mut layout = LayoutEngine::new(text_engine);
        layout
            .compute(reader.load(), LogicalViewport::new(200.0, 150.0).unwrap())
            .unwrap();
        (layout, window, div, text)
    }

    #[test]
    fn scene_plan_preserves_revision_and_parent_before_child_paint_order() {
        let (layout, window, div, text) = computed_fixture();
        let computed = layout.current().unwrap();

        let plan = ScenePlan::from_layout(computed, PhysicalSize::new(400, 300), 2.0).unwrap();

        assert_eq!(plan.revision(), computed.revision());
        assert_eq!(plan.viewport(), computed.viewport());
        assert_eq!(plan.physical_size(), PhysicalSize::new(400, 300));
        assert_eq!(plan.scale_factor(), 2.0);
        assert!(matches!(
            plan.items(),
            [
                PaintItem::Background { node: first, .. },
                PaintItem::Background { node: second, .. },
                PaintItem::Text { source }
            ] if *first == window && *second == div && *source == text
        ));
    }

    #[test]
    fn reverse_paint_order_hit_testing_prefers_the_deepest_box() {
        let (layout, _window, div, text) = computed_fixture();
        let plan =
            ScenePlan::from_layout(layout.current().unwrap(), PhysicalSize::new(200, 150), 1.0)
                .unwrap();

        assert_eq!(plan.hit_test(1.0, 1.0), Some(text));
        assert_eq!(plan.hit_test(99.0, 79.0), Some(div));
        assert_eq!(plan.hit_test(f32::NAN, 0.0), None);
    }

    #[test]
    fn rejects_empty_physical_targets_and_invalid_scale() {
        let (layout, ..) = computed_fixture();
        let computed = layout.current().unwrap();

        assert!(matches!(
            ScenePlan::from_layout(computed, PhysicalSize::new(0, 10), 1.0),
            Err(SceneError::EmptyTarget)
        ));
        assert!(matches!(
            ScenePlan::from_layout(computed, PhysicalSize::new(10, 10), f64::NAN),
            Err(SceneError::InvalidScaleFactor(_))
        ));
    }

    #[test]
    fn oversized_physical_targets_return_an_error() {
        let (layout, ..) = computed_fixture();
        let size = PhysicalSize::new(70_000, 10);

        assert!(matches!(
            ScenePlan::from_layout(layout.current().unwrap(), size, 1.0),
            Err(SceneError::TargetTooLarge {
                size: rejected,
                max_dimension: MAX_VELLO_SCENE_DIMENSION,
            }) if rejected == size
        ));
    }
}
