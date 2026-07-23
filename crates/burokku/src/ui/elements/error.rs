use thiserror::Error;

use crate::ui::elements::styles::StyleError;

#[derive(Debug, Error)]
pub enum DocumentError {
    #[error("Elements node {0} does not exist")]
    MissingNode(u64),
    #[error("Elements node {0} is not a text node")]
    NotText(u64),
    #[error("Elements::Body cannot be moved")]
    MoveBody,
    #[error("inserting node {child} below {parent} would create a cycle")]
    Cycle { parent: u64, child: u64 },
    #[error("node {anchor} is not a child of {parent}")]
    MissingAnchor { parent: u64, anchor: u64 },
    #[error("node {child} is not a child of {parent}")]
    NotAChild { parent: u64, child: u64 },
    #[error(transparent)]
    Style(#[from] StyleError),
}
