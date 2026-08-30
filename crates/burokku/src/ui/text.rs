//! UI-thread paragraph collection, shaping, caching, and glyph painting.
//!
//! Authoritative DOM nodes contain only strings and Burokku styles. All values
//! in this module are owned derivatives of one observed live-DOM revision and
//! remain on the UI thread.

#![allow(
    dead_code,
    reason = "cache diagnostics and structural glyph-batch accessors are regression-test APIs"
)]

mod collect;
mod engine;
mod error;
mod model;
pub(crate) mod paint;

pub(crate) use collect::collect_paragraph;
pub(crate) use engine::{ShapedParagraph, TextBrush, TextConstraint, TextEngine};
pub(crate) use error::TextError;
pub(crate) use model::{ParagraphInput, StyledTextRun, TextFingerprint};
