//! UI-thread lowering of the live DOM into owned Taffy layout state.

#![allow(
    dead_code,
    unused_imports,
    reason = "diagnostic and future incremental-topology APIs are retained and regression-tested"
)]

mod cache;
mod computed;
mod engine;
mod error;
mod reconcile;
mod topology;
mod tree;

pub(crate) use computed::{ComputedBox, ComputedLayout};
pub(crate) use engine::LayoutEngine;
pub(crate) use error::LayoutError;
pub(crate) use tree::{TextMeasureRequest, TextMeasurement, TextMeasurer};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LogicalViewport {
    width: f32,
    height: f32,
}

impl LogicalViewport {
    pub(crate) fn new(width: f32, height: f32) -> Result<Self, LayoutError> {
        if !width.is_finite() || !height.is_finite() || width < 0.0 || height < 0.0 {
            return Err(LayoutError::InvalidViewport { width, height });
        }
        Ok(Self {
            width: canonical_zero(width),
            height: canonical_zero(height),
        })
    }

    pub(crate) fn width(self) -> f32 {
        self.width
    }

    pub(crate) fn height(self) -> f32 {
        self.height
    }
}

fn canonical_zero(value: f32) -> f32 {
    if value == 0.0 {
        0.0
    } else {
        value
    }
}
