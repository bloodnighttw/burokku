use std::{collections::BTreeMap, sync::Arc};

use crate::ui::elements::{
    styles::{common::CommonStyle, window::WindowStyle},
    traits::Styles,
};

use self::styles::{color::RgbaColor, flex::FlexStyle, grid::GridStyle};
use slotmap::{new_key_type, SlotMap};
use thiserror::Error;

mod iter;
mod publication;

pub use iter::DomIter;
pub mod styles;
pub mod traits;

new_key_type! {
    /// A stable, generation-checked handle to a node in a [`Dom`].
    ///
    /// The handle remains valid when the arena grows, when the node moves in
    /// the tree, and in cloned DOM snapshots. Once its node is removed, the
    /// generation prevents the handle from referring to a later allocation that
    /// reuses the same slot.
    pub struct NodeId;
}

/// The data belonging to an element node.
///
/// This describes tags such as `<div>` and `<text>`. Text content is represented
/// separately by [`NodeKind::Text`], so an element and a DOM text node cannot be
/// confused with one another.
#[derive(Clone, Debug, PartialEq)]
pub enum Element {
    /// The `<window>` element. Only one may currently be attached to the app.
    Window { style: Box<WindowStyle> },
    /// A block `<div>` element.
    Div { style: Box<CommonStyle> },
    /// A flex container element.
    Flex { style: Box<FlexStyle> },
    /// A grid container element.
    Grid { style: Box<GridStyle> },
    /// A styled `<text>` element, distinct from a DOM text node.
    Text { style: Box<CommonStyle> },
}

impl Element {
    fn same_tag(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Self::Window { .. }, Self::Window { .. })
                | (Self::Div { .. }, Self::Div { .. })
                | (Self::Flex { .. }, Self::Flex { .. })
                | (Self::Grid { .. }, Self::Grid { .. })
                | (Self::Text { .. }, Self::Text { .. })
        )
    }

    // TODO: make Styles trait use self.supports_property() instead of static methods
    fn supports_style_property(&self, name: &str) -> bool {
        match self {
            Self::Div { .. } => CommonStyle::supports_property(name),
            Self::Window { .. } => WindowStyle::supports_property(name),
            Self::Flex { .. } => FlexStyle::supports_property(name),
            Self::Grid { .. } => GridStyle::supports_property(name),
            Self::Text { .. } => CommonStyle::supports_property(name),
        }
    }

    fn set_style_property(&mut self, name: &str, value: &str) -> bool {
        match self {
            Self::Div { style } | Self::Text { style } => style.set_property(name, value),
            Self::Window { style } => style.set_property(name, value),
            Self::Flex { style } => style.set_property(name, value),
            Self::Grid { style } => style.set_property(name, value),
        }
    }

    fn remove_style_property(&mut self, name: &str) -> bool {
        match self {
            Self::Div { style } | Self::Text { style } => style.remove_property(name),
            Self::Window { style } => style.remove_property(name),
            Self::Flex { style } => style.remove_property(name),
            Self::Grid { style } => style.remove_property(name),
        }
    }

    pub fn background_color(&self) -> Option<RgbaColor> {
        match self {
            Self::Window { style } => style.background_color,
            Self::Div { style } | Self::Text { style } => style.background_color,
            Self::Flex { style } => style.common.background_color,
            Self::Grid { style } => style.common.background_color,
        }
    }
}

/// The immutable kind of a DOM node.
///
/// Parent and child relationships deliberately live in [`Node`] rather than in
/// this enum. The app root is created internally by [`Dom::new`]; callers create
/// only element and text nodes through the corresponding typed constructors.
#[derive(Clone, Debug, PartialEq)]
pub enum NodeKind {
    App,
    Element(Element),
    Text(String),
}

impl NodeKind {
    /// Returns true if `child` is a valid child of `self`.
    fn accepts(&self, child: &Self) -> bool {
        match self {
            Self::App => matches!(child, Self::Element(Element::Window { .. })),
            Self::Element(
                Element::Window { .. }
                | Element::Div { .. }
                | Element::Flex { .. }
                | Element::Grid { .. },
            ) => matches!(
                child,
                Self::Element(
                    Element::Div { .. }
                        | Element::Flex { .. }
                        | Element::Grid { .. }
                        | Element::Text { .. }
                ) | Self::Text(_)
            ),
            // A styled <text> is a rich-text container: raw text nodes hold its
            // content, while nested <text> elements introduce styled text runs.
            Self::Element(Element::Text { .. }) => {
                matches!(child, Self::Text(_) | Self::Element(Element::Text { .. }))
            }
            Self::Text(_) => false,
        }
    }

    pub fn as_element(&self) -> Option<&Element> {
        match self {
            Self::Element(element) => Some(element),
            Self::App | Self::Text(_) => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            Self::App | Self::Element(_) => None,
        }
    }
}

/// Revisions for independently cached parts of a node.
///
/// Consumers compare these values with the revisions used to build their
/// computed state. This allows style or content changes to be processed
/// without treating every DOM update as a complete structural rebuild.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NodeRevisions {
    pub structure: u64,
    pub style: u64,
    pub content: u64,
}

/// An arena entry containing a node kind and its tree relationships.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    kind: NodeKind,
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    attributes: BTreeMap<String, String>,
    revisions: NodeRevisions,
}

impl Node {
    pub fn kind(&self) -> &NodeKind {
        &self.kind
    }

    pub fn element(&self) -> Option<&Element> {
        self.kind.as_element()
    }

    pub fn text(&self) -> Option<&str> {
        self.kind.as_text()
    }

    pub fn parent(&self) -> Option<NodeId> {
        self.parent
    }

    pub fn children(&self) -> &[NodeId] {
        &self.children
    }

    pub fn attributes(&self) -> &BTreeMap<String, String> {
        &self.attributes
    }

    pub fn revisions(&self) -> NodeRevisions {
        self.revisions
    }
}

/// The mutable DOM arena.
///
/// `Dom` starts with an `App` root. Nodes may be created while detached and
/// then inserted with [`Dom::append_child`] or [`Dom::insert_child`]. Structural
/// operations are validated before mutation, so an invalid operation leaves
/// the tree unchanged.
#[derive(Clone, Debug)]
pub struct Dom {
    // Cloning the arena for publication only clones these pointers. Node data
    // is copied lazily by `node_mut` when a published node is changed.
    nodes: SlotMap<NodeId, Arc<Node>>,
    root: NodeId,
    revision: u64,
}

impl Default for Dom {
    fn default() -> Self {
        Self::new()
    }
}

impl Dom {
    pub fn new() -> Self {
        let mut nodes = SlotMap::with_key();
        let root = nodes.insert(Arc::new(Node {
            kind: NodeKind::App,
            parent: None,
            children: Vec::new(),
            attributes: BTreeMap::new(),
            revisions: NodeRevisions::default(),
        }));

        Self {
            nodes,
            root,
            revision: 0,
        }
    }

    pub fn root(&self) -> NodeId {
        self.root
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn contains(&self, id: NodeId) -> bool {
        self.nodes.contains_key(id)
    }

    #[cfg(test)]
    pub(crate) fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id).map(Arc::as_ref)
    }

    pub fn kind(&self, id: NodeId) -> Option<&NodeKind> {
        self.node(id).map(Node::kind)
    }

    pub fn element(&self, id: NodeId) -> Option<&Element> {
        self.node(id)?.element()
    }

    pub fn text(&self, id: NodeId) -> Option<&str> {
        self.node(id)?.text()
    }

    pub fn attribute(&self, id: NodeId, name: &str) -> Option<&str> {
        self.node(id)?.attributes.get(name).map(String::as_str)
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).and_then(Node::parent)
    }

    pub fn children(&self, id: NodeId) -> Option<&[NodeId]> {
        self.node(id).map(Node::children)
    }

    pub fn supports_style_property(&self, id: NodeId, name: &str) -> Result<bool, DomError> {
        let node = self.node(id).ok_or(DomError::NodeNotFound(id))?;
        Ok(node
            .element()
            .is_some_and(|element| element.supports_style_property(name)))
    }

    pub fn set_style_property(
        &mut self,
        id: NodeId,
        name: &str,
        value: &str,
    ) -> Result<bool, DomError> {
        let node = self.node(id).ok_or(DomError::NodeNotFound(id))?;
        let Some(current) = node.element() else {
            return Ok(false);
        };
        let mut element = current.clone();
        if !element.set_style_property(name, value) {
            return Ok(false);
        }
        if &element == current {
            return Ok(true);
        }

        let node = self.node_mut(id)?;
        node.kind = NodeKind::Element(element);
        bump(&mut node.revisions.style);
        self.bump_revision();
        Ok(true)
    }

    pub fn remove_style_property(&mut self, id: NodeId, name: &str) -> Result<bool, DomError> {
        let node = self.node(id).ok_or(DomError::NodeNotFound(id))?;
        let Some(current) = node.element() else {
            return Ok(false);
        };
        let mut element = current.clone();
        if !element.remove_style_property(name) {
            return Ok(false);
        }
        if &element == current {
            return Ok(true);
        }

        let node = self.node_mut(id)?;
        node.kind = NodeKind::Element(element);
        bump(&mut node.revisions.style);
        self.bump_revision();
        Ok(true)
    }

    /// Allocates a detached element node and returns its stable handle.
    pub fn create_element(&mut self, element: Element) -> NodeId {
        self.create_node(NodeKind::Element(element))
    }

    /// Allocates a detached DOM text node and returns its stable handle.
    pub fn create_text(&mut self, text: impl Into<String>) -> NodeId {
        self.create_node(NodeKind::Text(text.into()))
    }

    fn create_node(&mut self, kind: NodeKind) -> NodeId {
        let id = self.nodes.insert(Arc::new(Node {
            kind,
            parent: None,
            children: Vec::new(),
            attributes: BTreeMap::new(),
            revisions: NodeRevisions::default(),
        }));
        self.bump_revision();
        id
    }

    /// Sets an element attribute, returning without a revision change when the
    /// value is already present.
    pub fn set_attribute(
        &mut self,
        id: NodeId,
        name: String,
        value: String,
    ) -> Result<(), DomError> {
        let node = self.node(id).ok_or(DomError::NodeNotFound(id))?;
        if node.element().is_none() {
            return Err(DomError::NodeNotElement(id));
        }
        if node.attributes.get(&name) == Some(&value) {
            return Ok(());
        }
        let node = self.node_mut(id)?;
        node.attributes.insert(name, value);
        bump(&mut node.revisions.content);
        self.bump_revision();
        Ok(())
    }

    pub fn remove_attribute(&mut self, id: NodeId, name: &str) -> Result<Option<String>, DomError> {
        let node = self.node(id).ok_or(DomError::NodeNotFound(id))?;
        if node.element().is_none() {
            return Err(DomError::NodeNotElement(id));
        }
        if !node.attributes.contains_key(name) {
            return Ok(None);
        }
        let node = self.node_mut(id)?;
        let removed = node.attributes.remove(name);
        bump(&mut node.revisions.content);
        self.bump_revision();
        Ok(removed)
    }

    /// Replaces an element's data without changing its tag or stable handle.
    pub fn set_element(&mut self, id: NodeId, element: Element) -> Result<(), DomError> {
        let node = self.node(id).ok_or(DomError::NodeNotFound(id))?;
        let current = node.element().ok_or(DomError::NodeNotElement(id))?;
        if !current.same_tag(&element) {
            return Err(DomError::ElementTagMismatch(id));
        }
        if current == &element {
            return Ok(());
        }

        let node = self.node_mut(id)?;
        node.kind = NodeKind::Element(element);
        bump(&mut node.revisions.style);
        self.bump_revision();
        Ok(())
    }

    /// Replaces a text node's content without changing its stable handle.
    pub fn set_text(&mut self, id: NodeId, text: impl Into<String>) -> Result<(), DomError> {
        let text = text.into();
        let node = self.node(id).ok_or(DomError::NodeNotFound(id))?;
        let current = node.text().ok_or(DomError::NodeNotText(id))?;
        if current == text {
            return Ok(());
        }

        let node = self.node_mut(id)?;
        node.kind = NodeKind::Text(text);
        bump(&mut node.revisions.content);
        self.bump_revision();
        Ok(())
    }

    pub fn append_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), DomError> {
        let child_count = self
            .node(parent)
            .ok_or(DomError::NodeNotFound(parent))?
            .children
            .len();
        let index = if self.parent(child) == Some(parent) {
            child_count.saturating_sub(1)
        } else {
            child_count
        };
        self.insert_child(parent, index, child)
    }

    /// Inserts or moves `child` to `index` in `parent`'s child list.
    ///
    /// `index` is interpreted against the final list (after removing `child`
    /// from its old position, if this is a move within the same parent).
    pub fn insert_child(
        &mut self,
        parent: NodeId,
        index: usize,
        child: NodeId,
    ) -> Result<(), DomError> {
        let parent_node = self.node(parent).ok_or(DomError::NodeNotFound(parent))?;
        let child_node = self.node(child).ok_or(DomError::NodeNotFound(child))?;

        if child == self.root || matches!(child_node.kind, NodeKind::App) {
            return Err(DomError::AppMustBeRoot);
        }
        if !parent_node.kind.accepts(&child_node.kind) {
            return Err(DomError::InvalidRelationship { parent, child });
        }
        if self.is_ancestor_or_self(child, parent) {
            return Err(DomError::Cycle { parent, child });
        }

        let same_parent = child_node.parent == Some(parent);
        let current_index = same_parent.then(|| {
            parent_node
                .children
                .iter()
                .position(|existing| *existing == child)
                .expect("a child's parent contains the child")
        });
        let final_len = parent_node.children.len() - usize::from(same_parent);
        if index > final_len {
            return Err(DomError::IndexOutOfBounds {
                parent,
                index,
                len: final_len,
            });
        }
        if matches!(parent_node.kind, NodeKind::App)
            && parent_node
                .children
                .iter()
                .any(|existing| *existing != child)
        {
            return Err(DomError::AppAlreadyHasWindow);
        }

        if current_index == Some(index) {
            return Ok(());
        }

        let old_parent = child_node.parent;
        if same_parent {
            let parent_node = self.node_mut(parent)?;
            parent_node.children.retain(|existing| *existing != child);
            parent_node.children.insert(index, child);
            bump(&mut parent_node.revisions.structure);
        } else {
            if let Some(old_parent) = old_parent {
                let old_parent_node = self.node_mut(old_parent)?;
                old_parent_node
                    .children
                    .retain(|existing| *existing != child);
                bump(&mut old_parent_node.revisions.structure);
            }

            let parent_node = self.node_mut(parent)?;
            parent_node.children.insert(index, child);
            bump(&mut parent_node.revisions.structure);

            let child_node = self.node_mut(child)?;
            child_node.parent = Some(parent);
            bump(&mut child_node.revisions.structure);
        }
        self.bump_revision();
        Ok(())
    }

    /// Detaches a node without invalidating its handle or those of descendants.
    pub fn detach(&mut self, id: NodeId) -> Result<(), DomError> {
        if id == self.root {
            return Err(DomError::CannotDetachRoot);
        }
        let parent = self.node(id).ok_or(DomError::NodeNotFound(id))?.parent;
        if let Some(parent) = parent {
            let parent_node = self.node_mut(parent)?;
            parent_node.children.retain(|child| *child != id);
            bump(&mut parent_node.revisions.structure);

            let node = self.node_mut(id)?;
            node.parent = None;
            bump(&mut node.revisions.structure);
            self.bump_revision();
        }
        Ok(())
    }

    /// Removes a node and all descendants, invalidating all of their handles.
    pub fn remove_subtree(&mut self, id: NodeId) -> Result<NodeKind, DomError> {
        if id == self.root {
            return Err(DomError::CannotRemoveRoot);
        }
        let parent = self.node(id).ok_or(DomError::NodeNotFound(id))?.parent;
        if let Some(parent) = parent {
            let parent_node = self.node_mut(parent)?;
            parent_node.children.retain(|child| *child != id);
            bump(&mut parent_node.revisions.structure);
        }

        let removed = self
            .nodes
            .remove(id)
            .expect("the node was checked immediately before removal");
        let kind = removed.kind.clone();
        let mut pending = removed.children.clone();
        while let Some(descendant) = pending.pop() {
            if let Some(node) = self.nodes.remove(descendant) {
                pending.extend(node.children.iter().copied());
            }
        }
        self.bump_revision();
        Ok(kind)
    }

    /// Iterates over the reachable tree in pre-order, yielding stable IDs.
    pub fn iter(&self) -> DomIter<'_> {
        DomIter::new(self)
    }

    fn node_mut(&mut self, id: NodeId) -> Result<&mut Node, DomError> {
        let node = self.nodes.get_mut(id).ok_or(DomError::NodeNotFound(id))?;
        Ok(Arc::make_mut(node))
    }

    #[cfg(test)]
    fn shares_node_with(&self, other: &Self, id: NodeId) -> bool {
        match (self.nodes.get(id), other.nodes.get(id)) {
            (Some(left), Some(right)) => Arc::ptr_eq(left, right),
            _ => false,
        }
    }

    fn is_ancestor_or_self(&self, possible_ancestor: NodeId, mut id: NodeId) -> bool {
        loop {
            if id == possible_ancestor {
                return true;
            }
            match self.nodes[id].parent {
                Some(parent) => id = parent,
                None => return false,
            }
        }
    }

    fn bump_revision(&mut self) {
        bump(&mut self.revision);
    }
}

// increments a revision counter
fn bump(revision: &mut u64) {
    *revision = revision
        // u64 is so large so it should never overflow
        // but if it overflows, it also should not break anything
        // since revision same from 0 to overflow 0 should be very
        // very unlikely to happen.
        .wrapping_add(1)
}

impl<'a> IntoIterator for &'a Dom {
    type Item = (NodeId, &'a NodeKind);
    type IntoIter = DomIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum DomError {
    #[error("node {0:?} does not exist or is stale")]
    NodeNotFound(NodeId),
    #[error("App must be the root node")]
    AppMustBeRoot,
    #[error("node {0:?} is not an element")]
    NodeNotElement(NodeId),
    #[error("node {0:?} is not a text node")]
    NodeNotText(NodeId),
    #[error("node {0:?}'s element tag cannot be changed")]
    ElementTagMismatch(NodeId),
    #[error("cannot insert node {child:?} under node {parent:?}")]
    InvalidRelationship { parent: NodeId, child: NodeId },
    #[error("only one Window may be attached to App")]
    AppAlreadyHasWindow,
    #[error("inserting node {child:?} under node {parent:?} would create a cycle")]
    Cycle { parent: NodeId, child: NodeId },
    #[error("child index {index} is out of bounds for node {parent:?} with length {len}")]
    IndexOutOfBounds {
        parent: NodeId,
        index: usize,
        len: usize,
    },
    #[error("the root node cannot be detached")]
    CannotDetachRoot,
    #[error("the root node cannot be removed")]
    CannotRemoveRoot,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_survive_moves_updates_and_snapshot_clones() {
        let mut dom = Dom::new();
        let window = dom.create_element(Element::Window {
            style: Box::default(),
        });
        let first_parent = dom.create_element(Element::Div {
            style: Box::default(),
        });
        let second_parent = dom.create_element(Element::Div {
            style: Box::default(),
        });
        let child = dom.create_element(Element::Text {
            style: Box::default(),
        });

        dom.append_child(dom.root(), window).unwrap();
        dom.append_child(window, first_parent).unwrap();
        dom.append_child(window, second_parent).unwrap();
        dom.append_child(first_parent, child).unwrap();
        dom.append_child(second_parent, child).unwrap();
        dom.set_element(
            child,
            Element::Text {
                style: Box::new(CommonStyle {
                    background_color: Some(RgbaColor::rgb(1, 2, 3)),
                    ..CommonStyle::default()
                }),
            },
        )
        .unwrap();

        assert_eq!(dom.parent(child), Some(second_parent));
        assert!(matches!(dom.element(child), Some(Element::Text { .. })));

        let snapshot = dom.clone();
        assert!(matches!(
            snapshot.element(child),
            Some(Element::Text { .. })
        ));
        assert_eq!(snapshot.parent(child), Some(second_parent));
    }

    #[test]
    fn removed_handles_never_refer_to_reused_slots() {
        let mut dom = Dom::new();
        let stale = dom.create_element(Element::Div {
            style: Box::default(),
        });
        dom.remove_subtree(stale).unwrap();
        let replacement = dom.create_element(Element::Div {
            style: Box::default(),
        });

        assert_ne!(stale, replacement);
        assert!(!dom.contains(stale));
        assert!(dom.contains(replacement));
        assert_eq!(
            dom.set_element(
                stale,
                Element::Div {
                    style: Box::default(),
                },
            ),
            Err(DomError::NodeNotFound(stale))
        );
    }

    #[test]
    fn invalid_mutations_leave_the_tree_unchanged() {
        let mut dom = Dom::new();
        let window = dom.create_element(Element::Window {
            style: Box::default(),
        });
        let div = dom.create_element(Element::Div {
            style: Box::default(),
        });
        let text = dom.create_element(Element::Text {
            style: Box::default(),
        });
        dom.append_child(dom.root(), window).unwrap();
        dom.append_child(window, div).unwrap();
        dom.append_child(div, text).unwrap();
        let revision = dom.revision();

        assert!(matches!(
            dom.append_child(text, div),
            Err(DomError::InvalidRelationship { .. }) | Err(DomError::Cycle { .. })
        ));
        assert_eq!(dom.parent(div), Some(window));
        assert_eq!(dom.parent(text), Some(div));
        assert_eq!(dom.revision(), revision);
    }

    #[test]
    fn reports_supported_style_properties_by_element_type() {
        let mut dom = Dom::new();
        let window = dom.create_element(Element::Window {
            style: Box::default(),
        });
        let div = dom.create_element(Element::Div {
            style: Box::default(),
        });
        let flex = dom.create_element(Element::Flex {
            style: Box::default(),
        });
        let grid = dom.create_element(Element::Grid {
            style: Box::default(),
        });
        let text_element = dom.create_element(Element::Text {
            style: Box::default(),
        });
        let text_node = dom.create_text("");

        for id in [window, div, flex, grid, text_element] {
            assert_eq!(dom.supports_style_property(id, "width"), Ok(true));
            assert_eq!(dom.supports_style_property(id, "not-defined"), Ok(false));
        }

        assert_eq!(dom.supports_style_property(div, "gap"), Ok(false));
        assert_eq!(dom.supports_style_property(flex, "gap"), Ok(true));
        assert_eq!(
            dom.supports_style_property(flex, "justify-items"),
            Ok(false)
        );
        assert_eq!(dom.supports_style_property(grid, "justify-items"), Ok(true));
        assert_eq!(
            dom.supports_style_property(grid, "flex-direction"),
            Ok(false)
        );
        assert_eq!(dom.supports_style_property(dom.root(), "width"), Ok(false));
        assert_eq!(dom.supports_style_property(text_node, "width"), Ok(false));
    }

    #[test]
    fn style_no_ops_do_not_advance_revisions() {
        let mut dom = Dom::new();
        let div = dom.create_element(Element::Div {
            style: Box::default(),
        });
        let initial_revision = dom.revision();

        assert_eq!(dom.set_style_property(div, "flex-grow", "0.00"), Ok(true));
        assert_eq!(dom.revision(), initial_revision);

        assert_eq!(dom.set_style_property(div, "flex-grow", "1"), Ok(true));
        let changed_revision = dom.revision();
        assert!(changed_revision > initial_revision);
        assert_eq!(dom.set_style_property(div, "flex-grow", "1.0"), Ok(true));
        assert_eq!(dom.revision(), changed_revision);

        assert_eq!(dom.remove_style_property(div, "flex-grow"), Ok(true));
        let removed_revision = dom.revision();
        assert_eq!(dom.remove_style_property(div, "flex-grow"), Ok(true));
        assert_eq!(dom.revision(), removed_revision);
    }

    #[test]
    fn tracks_style_and_text_revisions_independently() {
        let mut dom = Dom::new();
        let flex = dom.create_element(Element::Flex {
            style: Box::default(),
        });
        let text = dom.create_text("before");

        let style = FlexStyle {
            common: CommonStyle {
                flex_grow: 1.0,
                ..CommonStyle::default()
            },
            ..FlexStyle::default()
        };
        dom.set_element(
            flex,
            Element::Flex {
                style: Box::new(style),
            },
        )
        .unwrap();
        dom.set_text(text, "after").unwrap();

        assert_eq!(
            dom.node(flex).unwrap().revisions(),
            NodeRevisions {
                style: 1,
                ..NodeRevisions::default()
            }
        );
        assert_eq!(
            dom.node(text).unwrap().revisions(),
            NodeRevisions {
                content: 1,
                ..NodeRevisions::default()
            }
        );
    }

    #[test]
    fn structural_mutations_mark_only_affected_nodes() {
        let mut dom = Dom::new();
        let window = dom.create_element(Element::Window {
            style: Box::default(),
        });
        let first = dom.create_element(Element::Div {
            style: Box::default(),
        });
        let second = dom.create_element(Element::Div {
            style: Box::default(),
        });
        dom.append_child(dom.root(), window).unwrap();
        dom.append_child(window, first).unwrap();
        dom.append_child(window, second).unwrap();

        let window_before = dom.node(window).unwrap().revisions();
        let first_before = dom.node(first).unwrap().revisions();
        dom.append_child(first, second).unwrap();

        assert_eq!(
            dom.node(window).unwrap().revisions().structure,
            window_before.structure + 1
        );
        assert_eq!(
            dom.node(first).unwrap().revisions().structure,
            first_before.structure + 1
        );
        assert_eq!(dom.node(second).unwrap().revisions().structure, 2);
        assert_eq!(dom.node(second).unwrap().revisions().style, 0);
        assert_eq!(dom.node(second).unwrap().revisions().content, 0);
    }

    #[test]
    fn no_op_replacement_does_not_advance_revisions() {
        let mut dom = Dom::new();
        let div = dom.create_element(Element::Div {
            style: Box::default(),
        });
        let text = dom.create_text("same");
        let dom_revision = dom.revision();
        let div_revisions = dom.node(div).unwrap().revisions();
        let text_revisions = dom.node(text).unwrap().revisions();

        dom.set_element(
            div,
            Element::Div {
                style: Box::default(),
            },
        )
        .unwrap();
        dom.set_text(text, "same").unwrap();

        assert_eq!(dom.revision(), dom_revision);
        assert_eq!(dom.node(div).unwrap().revisions(), div_revisions);
        assert_eq!(dom.node(text).unwrap().revisions(), text_revisions);
    }

    #[test]
    fn removing_a_subtree_invalidates_descendants() {
        let mut dom = Dom::new();
        let parent = dom.create_element(Element::Div {
            style: Box::default(),
        });
        let child = dom.create_text("content");
        dom.append_child(parent, child).unwrap();

        dom.remove_subtree(parent).unwrap();

        assert!(!dom.contains(parent));
        assert!(!dom.contains(child));
    }

    #[test]
    fn ordinary_elements_accept_text_nodes_directly() {
        let mut dom = Dom::new();
        let parents = [
            dom.create_element(Element::Window {
                style: Box::default(),
            }),
            dom.create_element(Element::Div {
                style: Box::default(),
            }),
            dom.create_element(Element::Flex {
                style: Box::default(),
            }),
            dom.create_element(Element::Grid {
                style: Box::default(),
            }),
        ];

        for parent in parents {
            let text = dom.create_text("content");
            dom.append_child(parent, text).unwrap();
            assert_eq!(dom.parent(text), Some(parent));
            assert_eq!(dom.text(text), Some("content"));
        }
    }

    #[test]
    fn styled_text_elements_accept_text_nodes_and_nested_text_elements() {
        // Represents: <text>text here<text>asdad</text></text>
        let mut dom = Dom::new();
        let outer = dom.create_element(Element::Text {
            style: Box::default(),
        });
        let outer_content = dom.create_text("text here");
        let inner = dom.create_element(Element::Text {
            style: Box::default(),
        });
        let inner_content = dom.create_text("asdad");

        dom.append_child(outer, outer_content).unwrap();
        dom.append_child(outer, inner).unwrap();
        dom.append_child(inner, inner_content).unwrap();

        assert_eq!(dom.children(outer), Some(&[outer_content, inner][..]));
        assert_eq!(dom.children(inner), Some(&[inner_content][..]));
    }

    #[test]
    fn text_nodes_are_leaves() {
        let mut dom = Dom::new();
        let parent = dom.create_text("parent");
        let child = dom.create_text("child");

        assert_eq!(
            dom.append_child(parent, child),
            Err(DomError::InvalidRelationship { parent, child })
        );
        assert_eq!(dom.parent(child), None);
    }

    #[test]
    fn node_kind_and_element_tag_are_immutable() {
        let mut dom = Dom::new();
        let div = dom.create_element(Element::Div {
            style: Box::default(),
        });
        let text = dom.create_text("content");

        assert_eq!(
            dom.set_element(
                div,
                Element::Flex {
                    style: Box::default(),
                },
            ),
            Err(DomError::ElementTagMismatch(div))
        );
        assert_eq!(
            dom.set_text(div, "changed"),
            Err(DomError::NodeNotText(div))
        );
        assert_eq!(
            dom.set_element(
                text,
                Element::Div {
                    style: Box::default(),
                },
            ),
            Err(DomError::NodeNotElement(text))
        );
        assert_eq!(
            dom.set_attribute(text, "role".into(), "note".into()),
            Err(DomError::NodeNotElement(text))
        );
        assert!(matches!(
            dom.kind(div),
            Some(NodeKind::Element(Element::Div { .. }))
        ));
        assert!(matches!(dom.kind(text), Some(NodeKind::Text(value)) if value == "content"));
    }
}
