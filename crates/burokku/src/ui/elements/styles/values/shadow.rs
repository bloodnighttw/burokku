use render::{BoxShadow, TextShadow};

use super::rgba;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Shadow {
    pub(crate) offset_x: f32,
    pub(crate) offset_y: f32,
    pub(crate) blur: f32,
    pub(crate) spread: f32,
    pub(crate) color: [u8; 4],
    pub(crate) inset: bool,
}

impl From<Shadow> for BoxShadow {
    fn from(value: Shadow) -> Self {
        Self {
            offset: [value.offset_x, value.offset_y],
            blur: value.blur,
            spread: value.spread,
            color: rgba(value.color),
            inset: value.inset,
        }
    }
}

impl From<Shadow> for TextShadow {
    fn from(value: Shadow) -> Self {
        Self {
            offset: [value.offset_x, value.offset_y],
            blur: value.blur,
            color: rgba(value.color),
        }
    }
}
