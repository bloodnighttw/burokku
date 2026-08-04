use render::GradientStop as RenderGradientStop;

use super::rgba;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GradientStop {
    pub(crate) color: [u8; 4],
    pub(crate) position: f32,
}

impl From<GradientStop> for RenderGradientStop {
    fn from(value: GradientStop) -> Self {
        Self {
            color: rgba(value.color),
            position: value.position,
        }
    }
}
