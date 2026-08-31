use thiserror::Error;

use crate::ui::{elements::NodeId, text::TextError};

use super::topology::LayoutId;

#[derive(Debug, Error)]
pub(crate) enum LayoutError {
    #[error(transparent)]
    Text(#[from] TextError),

    #[error("logical viewport must be finite and non-negative, got {width}x{height}")]
    InvalidViewport { width: f32, height: f32 },

    #[error("the committed App root is missing or has the wrong node kind")]
    InvalidAppRoot,

    #[error("App must have zero or one Window child, found {count}")]
    InvalidAppChildren { count: usize },

    #[error("App child {0:?} is not a Window element")]
    ExpectedWindow(NodeId),

    #[error("Window element {0:?} may only appear at the layout root")]
    UnexpectedWindow(NodeId),

    #[error("committed node {0:?} is missing or stale")]
    MissingDomNode(NodeId),

    #[error("committed child {child:?} does not point back to parent {parent:?}")]
    InvalidDomRelationship { parent: NodeId, child: NodeId },

    #[error("reachable DOM node {0:?} occurs more than once")]
    DuplicateDomNode(NodeId),

    #[error("raw text node {0:?} is attached outside an outer text paragraph")]
    RawTextOutsideParagraph(NodeId),

    #[error("paragraph {paragraph:?} contains invalid descendant {child:?}")]
    InvalidParagraphChild { paragraph: NodeId, child: NodeId },

    #[error("layout ID {0:?} has no topology entry")]
    MissingLayoutNode(LayoutId),

    #[error("layout node {0:?} has more than one effective parent")]
    MultipleLayoutParents(LayoutId),

    #[error("layout topology contains a cycle through {0:?}")]
    LayoutCycle(LayoutId),

    #[error("layout node {0:?} is not reachable from the Window root")]
    UnreachableLayoutNode(LayoutId),

    #[error("layout node {0:?} has no computed sidecar")]
    MissingLayoutSidecar(LayoutId),

    #[error("layout container depth {depth} exceeds hard limit {limit}")]
    TreeTooDeep { depth: usize, limit: usize },

    #[error("paragraph {paragraph:?} measurement failed: {message}")]
    TextMeasurement { paragraph: NodeId, message: String },

    #[error("text measurement generation changed during layout from {before} to {after}")]
    TextGenerationChanged { before: u64, after: u64 },

    #[error("paragraph {0:?} has a missing, stale, or mismatched final shaped paragraph")]
    InvalidFinalParagraph(NodeId),

    #[error("paragraph {paragraph:?} returned invalid {field} value {value}")]
    InvalidTextMetric {
        paragraph: NodeId,
        field: &'static str,
        value: f32,
    },

    #[error("layout for node {node:?} contains invalid {field} value {value}")]
    InvalidComputedValue {
        node: NodeId,
        field: &'static str,
        value: f32,
    },
}
