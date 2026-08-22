use thiserror::Error;

use crate::ui::elements::NodeId;

/// Failure while deriving or computing MTS text state from a committed DOM.
#[derive(Clone, Debug, Error, PartialEq)]
pub(crate) enum TextError {
    #[error("committed text node {0:?} is missing or stale")]
    MissingNode(NodeId),

    #[error("node {0:?} is not an outer styled text paragraph")]
    ExpectedParagraph(NodeId),

    #[error("committed text child {child:?} does not point back to parent {parent:?}")]
    InvalidRelationship { parent: NodeId, child: NodeId },

    #[error("paragraph {paragraph:?} contains duplicate descendant {node:?}")]
    DuplicateNode { paragraph: NodeId, node: NodeId },

    #[error("paragraph tree exceeds the supported depth of {limit} at node {node:?}")]
    TreeTooDeep { node: NodeId, limit: usize },

    #[error("paragraph {paragraph:?} contains invalid descendant {child:?}")]
    InvalidParagraphChild { paragraph: NodeId, child: NodeId },

    #[error("paragraph {paragraph:?} has invalid styled-run coverage: {reason}")]
    InvalidRunCoverage {
        paragraph: NodeId,
        reason: &'static str,
    },

    #[error("text width constraint must be finite and non-negative, got {width}")]
    InvalidConstraint { width: f32 },

    #[error("font data does not contain a usable OpenType font")]
    InvalidFontData,

    #[error("paragraph {paragraph:?} has {count} styled runs, exceeding the supported limit")]
    TooManyStyledRuns { paragraph: NodeId, count: usize },

    #[error("paragraph {0:?} could not resolve a usable font")]
    MissingUsableFont(NodeId),

    #[error("paragraph {paragraph:?} produced invalid {field} metric {value}")]
    InvalidMetric {
        paragraph: NodeId,
        field: &'static str,
        value: f32,
    },
}
