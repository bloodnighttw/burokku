//! MTS-owned paragraph collection, shaping, caching, and glyph painting.
//!
//! Authoritative DOM snapshots contain only strings and Burokku styles. All
//! values in this module are derived from one immutable snapshot and remain on
//! the main thread.

#![allow(
    dead_code,
    reason = "the glyph adapter is connected to the native scene host in Problem 8"
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
