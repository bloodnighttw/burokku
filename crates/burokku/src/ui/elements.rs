use std::collections::{BTreeMap, HashSet};

use crate::ui::elements::{
    styles::{common::CommonStyle, text::TextElementStyle, window::WindowStyle},
    traits::Styles,
};

use self::styles::{color::RgbaColor, flex::FlexStyle, grid::GridStyle};
use slotmap::{new_key_type, SlotMap};
use thiserror::Error;

mod iter;
pub use iter::DomIter;
pub mod styles;
pub mod traits;

new_key_type! {
    /// A stable, generation-checked handle to a node in a [`Dom`].
    ///
    /// The handle remains valid when the arena grows, when the node moves in
    /// the tree. Once its node is removed, the generation prevents the handle
    /// from referring to a later allocation that reuses the same slot.
    pub struct NodeId;
}

/// A script-creatable element tag supported by Burokku.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ElementTag {
    Window,
    Div,
    Flex,
    Grid,
    Text,
}

impl ElementTag {
    pub const fn local_name(self) -> &'static str {
        match self {
            Self::Window => "window",
            Self::Div => "div",
            Self::Flex => "flex",
            Self::Grid => "grid",
            Self::Text => "text",
        }
    }
}

impl TryFrom<&str> for ElementTag {
    type Error = ElementTagError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "window" => Ok(Self::Window),
            "div" => Ok(Self::Div),
            "flex" => Ok(Self::Flex),
            "grid" => Ok(Self::Grid),
            "text" => Ok(Self::Text),
            _ => Err(ElementTagError(value.into())),
        }
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("unsupported Burokku element tag {0:?}")]
pub struct ElementTagError(pub String);

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
    Text { style: Box<TextElementStyle> },
}

impl Element {
    pub fn from_tag(tag: ElementTag) -> Self {
        match tag {
            ElementTag::Window => Self::Window {
                style: Box::default(),
            },
            ElementTag::Div => Self::Div {
                style: Box::default(),
            },
            ElementTag::Flex => Self::Flex {
                style: Box::default(),
            },
            ElementTag::Grid => Self::Grid {
                style: Box::default(),
            },
            ElementTag::Text => Self::Text {
                style: Box::default(),
            },
        }
    }

    pub const fn tag(&self) -> ElementTag {
        match self {
            Self::Window { .. } => ElementTag::Window,
            Self::Div { .. } => ElementTag::Div,
            Self::Flex { .. } => ElementTag::Flex,
            Self::Grid { .. } => ElementTag::Grid,
            Self::Text { .. } => ElementTag::Text,
        }
    }

    pub const fn local_name(&self) -> &'static str {
        self.tag().local_name()
    }

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
            Self::Text { .. } => TextElementStyle::supports_property(name),
        }
    }

    fn set_style_property(&mut self, name: &str, value: &str) -> bool {
        match self {
            Self::Div { style } => style.set_property(name, value),
            Self::Text { style } => style.set_property(name, value),
            Self::Window { style } => style.set_property(name, value),
            Self::Flex { style } => style.set_property(name, value),
            Self::Grid { style } => style.set_property(name, value),
        }
    }

    fn remove_style_property(&mut self, name: &str) -> bool {
        match self {
            Self::Div { style } => style.remove_property(name),
            Self::Text { style } => style.remove_property(name),
            Self::Window { style } => style.remove_property(name),
            Self::Flex { style } => style.remove_property(name),
            Self::Grid { style } => style.remove_property(name),
        }
    }

    pub fn background_color(&self) -> Option<RgbaColor> {
        match self {
            Self::Window { style } => style.background_color,
            Self::Div { style } => style.background_color,
            Self::Text { style } => style.common.background_color,
            Self::Flex { style } => style.common.background_color,
            Self::Grid { style } => style.common.background_color,
        }
    }
}

/// The immutable kind of a DOM node.
///
/// Parent and child relationships deliberately live in [`Node`] rather than in
/// this enum. The app root is created when the crate bootstraps the DOM;
/// callers create only element and text nodes through the corresponding typed
/// constructors.
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
                )
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

/// Nodes permanently removed by one detached-component lifetime sweep.
/// This is for cache cleanup in taffy layout tree/text layout tree.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ReclaimReport {
    pub(crate) roots: Vec<NodeId>,
    pub(crate) nodes: Vec<NodeId>,
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
///
/// Fresh DOM construction is crate-owned so an application cannot accidentally
/// create independent arenas whose [`NodeId`] values overlap.
#[derive(Debug)]
pub struct Dom {
    nodes: SlotMap<NodeId, Node>,
    root: NodeId,
    revision: u64,
}

impl Dom {
    /// Creates the application's DOM arena.
    ///
    /// This is crate-private to keep fresh arena creation under the application
    /// owner's control. Unit tests in this crate may create isolated arenas.
    ///
    /// this is to ensure only one instance of the DOM exists.
    pub(crate) fn new() -> Self {
        let mut nodes = SlotMap::with_key();
        let root = nodes.insert(Node {
            kind: NodeKind::App,
            parent: None,
            children: Vec::new(),
            attributes: BTreeMap::new(),
            revisions: NodeRevisions::default(),
        });

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
        self.nodes.get(id)
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

    pub fn element_tag(&self, id: NodeId) -> Result<ElementTag, DomError> {
        self.node(id)
            .ok_or(DomError::NodeNotFound(id))?
            .element()
            .map(Element::tag)
            .ok_or(DomError::NodeNotElement(id))
    }

    pub fn parent_node(&self, id: NodeId) -> Result<Option<NodeId>, DomError> {
        Ok(self.node(id).ok_or(DomError::NodeNotFound(id))?.parent)
    }

    pub fn first_child(&self, id: NodeId) -> Result<Option<NodeId>, DomError> {
        Ok(self
            .node(id)
            .ok_or(DomError::NodeNotFound(id))?
            .children
            .first()
            .copied())
    }

    pub fn last_child(&self, id: NodeId) -> Result<Option<NodeId>, DomError> {
        Ok(self
            .node(id)
            .ok_or(DomError::NodeNotFound(id))?
            .children
            .last()
            .copied())
    }

    pub fn next_sibling(&self, id: NodeId) -> Result<Option<NodeId>, DomError> {
        self.sibling_at_offset(id, 1)
    }

    pub fn previous_sibling(&self, id: NodeId) -> Result<Option<NodeId>, DomError> {
        self.sibling_at_offset(id, -1)
    }

    pub fn is_connected(&self, mut id: NodeId) -> Result<bool, DomError> {
        self.node(id).ok_or(DomError::NodeNotFound(id))?;
        loop {
            if id == self.root {
                return Ok(true);
            }
            match self.nodes[id].parent {
                Some(parent) => id = parent,
                None => return Ok(false),
            }
        }
    }

    pub fn contains_node(
        &self,
        ancestor: NodeId,
        mut descendant: NodeId,
    ) -> Result<bool, DomError> {
        self.node(ancestor)
            .ok_or(DomError::NodeNotFound(ancestor))?;
        self.node(descendant)
            .ok_or(DomError::NodeNotFound(descendant))?;

        loop {
            if descendant == ancestor {
                return Ok(true);
            }
            match self.nodes[descendant].parent {
                Some(parent) => descendant = parent,
                None => return Ok(false),
            }
        }
    }

    pub fn text_content(&self, id: NodeId) -> Result<String, DomError> {
        let node = self.node(id).ok_or(DomError::NodeNotFound(id))?;
        if let Some(text) = node.text() {
            return Ok(text.into());
        }

        let mut content = String::new();
        let mut pending = node.children.iter().rev().copied().collect::<Vec<_>>();
        while let Some(next) = pending.pop() {
            let node = self
                .node(next)
                .expect("all child handles belong to the same DOM arena");
            match node.kind() {
                NodeKind::Text(text) => content.push_str(text),
                NodeKind::App | NodeKind::Element(_) => {
                    pending.extend(node.children.iter().rev().copied());
                }
            }
        }
        Ok(content)
    }

    pub fn supports_style_property(&self, id: NodeId, name: &str) -> Result<bool, DomError> {
        let node = self.node(id).ok_or(DomError::NodeNotFound(id))?;
        Ok(node
            .element()
            .is_some_and(|element| element.supports_style_property(name)))
    }

    /// Sets a supported style property and reports whether its computed native
    /// value changed. Invalid values never enter authoritative DOM state.
    pub fn set_style_property(
        &mut self,
        id: NodeId,
        name: &str,
        value: &str,
    ) -> Result<bool, StyleError> {
        let node = self.node(id).ok_or(StyleError::NodeNotFound(id))?;
        let current = node.element().ok_or(StyleError::NodeNotElement(id))?;
        if !current.supports_style_property(name) {
            return Err(StyleError::UnsupportedProperty(name.into()));
        }

        let mut element = current.clone();
        if !element.set_style_property(name, value) {
            return Err(StyleError::InvalidValue {
                property: name.into(),
                value: value.into(),
            });
        }
        if &element == current {
            return Ok(false);
        }

        let node = self
            .node_mut(id)
            .map_err(|_| StyleError::NodeNotFound(id))?;
        node.kind = NodeKind::Element(element);
        bump(&mut node.revisions.style);
        self.bump_revision();
        Ok(true)
    }

    /// Removes a supported style property and reports whether its native value
    /// changed from the default.
    pub fn remove_style_property(&mut self, id: NodeId, name: &str) -> Result<bool, StyleError> {
        let node = self.node(id).ok_or(StyleError::NodeNotFound(id))?;
        let current = node.element().ok_or(StyleError::NodeNotElement(id))?;
        if !current.supports_style_property(name) {
            return Err(StyleError::UnsupportedProperty(name.into()));
        }

        let mut element = current.clone();
        debug_assert!(element.remove_style_property(name));
        if &element == current {
            return Ok(false);
        }

        let node = self
            .node_mut(id)
            .map_err(|_| StyleError::NodeNotFound(id))?;
        node.kind = NodeKind::Element(element);
        bump(&mut node.revisions.style);
        self.bump_revision();
        Ok(true)
    }

    /// Allocates a detached element node and returns its stable handle.
    pub fn create_element(&mut self, element: Element) -> NodeId {
        self.create_node(NodeKind::Element(element))
    }

    /// Allocates a detached element with the default data for `tag`.
    pub fn create_element_tag(&mut self, tag: ElementTag) -> NodeId {
        self.create_element(Element::from_tag(tag))
    }

    /// Allocates a detached DOM text node and returns its stable handle.
    pub fn create_text(&mut self, text: impl Into<String>) -> NodeId {
        self.create_node(NodeKind::Text(text.into()))
    }

    /// Internally creates a node of the given kind and returns its stable handle.
    /// Always prefers to use [`Self::create_element`] or [`Self::create_text`] instead.
    fn create_node(&mut self, kind: NodeKind) -> NodeId {
        let id = self.nodes.insert(Node {
            kind,
            parent: None,
            children: Vec::new(),
            attributes: BTreeMap::new(),
            revisions: NodeRevisions::default(),
        });
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
    pub fn set_text(&mut self, id: NodeId, text: impl Into<String>) -> Result<bool, DomError> {
        let text = text.into();
        let node = self.node(id).ok_or(DomError::NodeNotFound(id))?;
        let current = node.text().ok_or(DomError::NodeNotText(id))?;
        if current == text {
            return Ok(false);
        }

        let node = self.node_mut(id)?;
        node.kind = NodeKind::Text(text);
        bump(&mut node.revisions.content);
        self.bump_revision();
        Ok(true)
    }

    /// Replaces a styled text element's children with one raw text node.
    ///
    /// Existing children are detached rather than permanently removed so live
    /// JavaScript wrappers can continue to use them. Raw text nodes are updated
    /// in place. App and non-text elements reject assignment because raw text
    /// may only be attached beneath a styled text element.
    pub fn set_text_content(
        &mut self,
        id: NodeId,
        text: impl Into<String>,
    ) -> Result<bool, DomError> {
        let text = text.into();
        let node = self.node(id).ok_or(DomError::NodeNotFound(id))?;
        match node.kind() {
            NodeKind::Text(_) => return self.set_text(id, text),
            NodeKind::Element(Element::Text { .. }) => {}
            NodeKind::App | NodeKind::Element(_) => {
                return Err(DomError::TextContentNotSupported(id));
            }
        }

        if let [only_child] = node.children.as_slice() {
            if self.text(*only_child) == Some(text.as_str()) {
                return Ok(false);
            }
        }

        let old_children = node.children.clone();
        let text_id = self.nodes.insert(Node {
            kind: NodeKind::Text(text),
            parent: Some(id),
            children: Vec::new(),
            attributes: BTreeMap::new(),
            revisions: NodeRevisions {
                structure: 1,
                ..NodeRevisions::default()
            },
        });

        for child in old_children {
            let child = self.node_mut(child)?;
            child.parent = None;
            bump(&mut child.revisions.structure);
        }
        let node = self.node_mut(id)?;
        node.children.clear();
        node.children.push(text_id);
        bump(&mut node.revisions.structure);
        self.bump_revision();
        Ok(true)
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

    /// Inserts `child` immediately before `reference`, or appends it when the
    /// reference is `None`.
    pub fn insert_before(
        &mut self,
        parent: NodeId,
        child: NodeId,
        reference: Option<NodeId>,
    ) -> Result<(), DomError> {
        let Some(reference) = reference else {
            return self.append_child(parent, child);
        };

        let parent_node = self.node(parent).ok_or(DomError::NodeNotFound(parent))?;
        self.node(child).ok_or(DomError::NodeNotFound(child))?;
        self.node(reference)
            .ok_or(DomError::NodeNotFound(reference))?;
        if self.parent(reference) != Some(parent) {
            return Err(DomError::NotAChild {
                parent,
                child: reference,
            });
        }
        if child == reference {
            return Ok(());
        }

        let reference_index = parent_node
            .children
            .iter()
            .position(|candidate| *candidate == reference)
            .expect("a direct child is present in its parent's child list");
        let child_precedes_reference = self.parent(child) == Some(parent)
            && parent_node
                .children
                .iter()
                .position(|candidate| *candidate == child)
                .is_some_and(|child_index| child_index < reference_index);
        let final_index = reference_index - usize::from(child_precedes_reference);
        self.insert_child(parent, final_index, child)
    }

    /// Detaches `child` after verifying that it is a direct child of `parent`.
    pub fn remove_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), DomError> {
        self.node(parent).ok_or(DomError::NodeNotFound(parent))?;
        self.node(child).ok_or(DomError::NodeNotFound(child))?;
        if self.parent(child) != Some(parent) {
            return Err(DomError::NotAChild { parent, child });
        }
        self.detach(child)
    }

    /// Atomically replaces one direct child and leaves the old child detached.
    pub fn replace_child(
        &mut self,
        parent: NodeId,
        new_child: NodeId,
        old_child: NodeId,
    ) -> Result<(), DomError> {
        let parent_node = self.node(parent).ok_or(DomError::NodeNotFound(parent))?;
        let new_node = self
            .node(new_child)
            .ok_or(DomError::NodeNotFound(new_child))?;
        self.node(old_child)
            .ok_or(DomError::NodeNotFound(old_child))?;

        if self.parent(old_child) != Some(parent) {
            return Err(DomError::NotAChild {
                parent,
                child: old_child,
            });
        }
        if new_child == old_child {
            return Ok(());
        }
        if new_child == self.root || matches!(new_node.kind, NodeKind::App) {
            return Err(DomError::AppMustBeRoot);
        }
        if !parent_node.kind.accepts(&new_node.kind) {
            return Err(DomError::InvalidRelationship {
                parent,
                child: new_child,
            });
        }
        if self.is_ancestor_or_self(new_child, parent) {
            return Err(DomError::Cycle {
                parent,
                child: new_child,
            });
        }

        let old_index = parent_node
            .children
            .iter()
            .position(|candidate| *candidate == old_child)
            .expect("a direct child is present in its parent's child list");
        let new_parent = new_node.parent;
        let new_index = (new_parent == Some(parent)).then(|| {
            parent_node
                .children
                .iter()
                .position(|candidate| *candidate == new_child)
                .expect("a direct child is present in its parent's child list")
        });
        let final_old_index =
            old_index - usize::from(new_index.is_some_and(|index| index < old_index));

        if let Some(new_parent) = new_parent {
            let node = self.node_mut(new_parent)?;
            node.children.retain(|candidate| *candidate != new_child);
            bump(&mut node.revisions.structure);
        }

        let parent_node = self.node_mut(parent)?;
        parent_node.children[final_old_index] = new_child;
        if new_parent == Some(parent) {
            // Removing the existing sibling already bumped this same parent.
        } else {
            bump(&mut parent_node.revisions.structure);
        }

        if new_parent != Some(parent) {
            let new_node = self.node_mut(new_child)?;
            new_node.parent = Some(parent);
            bump(&mut new_node.revisions.structure);
        }
        let old_node = self.node_mut(old_child)?;
        old_node.parent = None;
        bump(&mut old_node.revisions.structure);

        self.bump_revision();
        Ok(())
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

    /// Permanently removes detached components not retained by a live wrapper.
    ///
    /// A wrapper for any node retains its complete component because parent and
    /// sibling traversal makes every node in that component observable.
    pub(crate) fn reclaim_unreachable_detached<I>(
        &mut self,
        wrapper_roots: I,
    ) -> Result<ReclaimReport, DomError>
    where
        I: IntoIterator<Item = NodeId>,
    {
        let mut marked = HashSet::new();
        self.mark_subtree(self.root, &mut marked);

        for mut wrapper_root in wrapper_roots {
            self.node(wrapper_root)
                .ok_or(DomError::NodeNotFound(wrapper_root))?;
            // Find the root of the component retained by this wrapper.
            while let Some(parent) = self.nodes[wrapper_root].parent {
                wrapper_root = parent;
            }
            self.mark_subtree(wrapper_root, &mut marked);
        }

        // filter that id is not the root and hasn't been visited, which
        // is recorded in the marked set, note this only records nodes that are
        // roots of unreachable subtrees (i.e., detached from the main tree).
        let unreachable_node_root = self
            .nodes
            .iter()
            .filter_map(|(id, node)| {
                (id != self.root && node.parent.is_none() && !marked.contains(&id)).then_some(id)
            })
            .collect::<Vec<_>>();
        let mut reclaimed = Vec::new();

        // this removes the unreachable subtrees from the arena
        for root in unreachable_node_root.iter().copied() {
            let mut pending = vec![root];
            while let Some(id) = pending.pop() {
                let Some(node) = self.nodes.remove(id) else {
                    continue;
                };
                pending.extend(node.children.iter().copied());
                reclaimed.push(id);
            }
        }

        if !reclaimed.is_empty() {
            self.bump_revision();
        }
        Ok(ReclaimReport {
            roots: unreachable_node_root,
            nodes: reclaimed,
        })
    }

    /// Iterates over the reachable tree in pre-order, yielding stable IDs.
    pub fn iter(&self) -> DomIter<'_> {
        DomIter::new(self)
    }

    fn node_mut(&mut self, id: NodeId) -> Result<&mut Node, DomError> {
        self.nodes.get_mut(id).ok_or(DomError::NodeNotFound(id))
    }

    fn sibling_at_offset(&self, id: NodeId, offset: isize) -> Result<Option<NodeId>, DomError> {
        let node = self.node(id).ok_or(DomError::NodeNotFound(id))?;
        let Some(parent) = node.parent else {
            return Ok(None);
        };
        let siblings = &self.nodes[parent].children;
        let index = siblings
            .iter()
            .position(|candidate| *candidate == id)
            .expect("a child's parent contains the child");
        let sibling_index = index.checked_add_signed(offset);
        Ok(sibling_index.and_then(|index| siblings.get(index).copied()))
    }

    // mark all nodes in subtree as reachable
    // hash set will mark nodes as visited
    fn mark_subtree(&self, root: NodeId, marked: &mut HashSet<NodeId>) {
        let mut pending = vec![root];
        while let Some(id) = pending.pop() {
            if !marked.insert(id) {
                continue;
            }
            pending.extend(self.nodes[id].children.iter().copied());
        }
    }

    // to prevent cycles
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

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum StyleError {
    #[error("node {0:?} does not exist or is stale")]
    NodeNotFound(NodeId),
    #[error("node {0:?} is not an element")]
    NodeNotElement(NodeId),
    #[error("unsupported style property {0:?}")]
    UnsupportedProperty(String),
    #[error("invalid value {value:?} for style property {property:?}")]
    InvalidValue { property: String, value: String },
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
    #[error("node {child:?} is not a direct child of node {parent:?}")]
    NotAChild { parent: NodeId, child: NodeId },
    #[error("node {0:?} does not support textContent assignment")]
    TextContentNotSupported(NodeId),
    #[error("the root node cannot be detached")]
    CannotDetachRoot,
    #[error("the root node cannot be removed")]
    CannotRemoveRoot,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_survive_moves_and_updates() {
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
                style: Box::new(TextElementStyle {
                    common: CommonStyle {
                        background_color: Some(RgbaColor::rgb(1, 2, 3)),
                        ..CommonStyle::default()
                    },
                    ..TextElementStyle::default()
                }),
            },
        )
        .unwrap();

        assert_eq!(dom.parent(child), Some(second_parent));
        assert!(matches!(dom.element(child), Some(Element::Text { .. })));
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

        for id in [div, flex, grid, text_element] {
            assert_eq!(dom.supports_style_property(id, "grid-row"), Ok(true));
            assert_eq!(dom.supports_style_property(id, "grid-column"), Ok(true));
            assert_eq!(dom.supports_style_property(id, "justify-self"), Ok(true));
        }
        assert_eq!(
            dom.supports_style_property(window, "justify-self"),
            Ok(false)
        );

        assert_eq!(dom.supports_style_property(div, "gap"), Ok(false));
        assert_eq!(dom.supports_style_property(flex, "gap"), Ok(true));
        assert_eq!(
            dom.supports_style_property(flex, "justify-items"),
            Ok(false)
        );
        assert_eq!(dom.supports_style_property(grid, "justify-items"), Ok(true));
        assert_eq!(
            dom.supports_style_property(grid, "grid-auto-flow"),
            Ok(true)
        );
        assert_eq!(
            dom.supports_style_property(div, "grid-auto-flow"),
            Ok(false)
        );
        assert_eq!(
            dom.supports_style_property(grid, "flex-direction"),
            Ok(false)
        );
        assert_eq!(dom.supports_style_property(dom.root(), "width"), Ok(false));
        assert_eq!(dom.supports_style_property(text_node, "width"), Ok(false));
    }

    #[test]
    fn text_elements_expose_box_and_typography_properties() {
        let mut dom = Dom::new();
        let div = dom.create_element(Element::Div {
            style: Box::default(),
        });
        let text = dom.create_element(Element::Text {
            style: Box::default(),
        });

        assert_eq!(dom.supports_style_property(text, "width"), Ok(true));
        assert_eq!(dom.supports_style_property(text, "font-size"), Ok(true));
        assert_eq!(dom.supports_style_property(div, "font-size"), Ok(false));
        assert_eq!(dom.set_style_property(text, "font-size", "20px"), Ok(true));
        assert_eq!(
            dom.set_style_property(text, "font-weight", "bold"),
            Ok(true)
        );

        let Some(Element::Text { style }) = dom.element(text) else {
            panic!("expected text element");
        };
        assert_eq!(style.text.font_size, Some(20.0));
        assert_eq!(style.text.font_weight, Some(styles::text::FontWeight::BOLD));

        assert_eq!(dom.remove_style_property(text, "font-size"), Ok(true));
        let Some(Element::Text { style }) = dom.element(text) else {
            panic!("expected text element");
        };
        assert_eq!(style.text.font_size, None);
    }

    #[test]
    fn grid_item_properties_apply_to_non_grid_elements() {
        let mut dom = Dom::new();
        let div = dom.create_element(Element::Div {
            style: Box::default(),
        });

        assert_eq!(
            dom.set_style_property(div, "grid-row", "2 / span 3"),
            Ok(true)
        );
        assert_eq!(
            dom.set_style_property(div, "justify-self", "center"),
            Ok(true)
        );

        let Some(Element::Div { style }) = dom.element(div) else {
            panic!("expected div element");
        };
        assert_eq!(
            style.item.grid_row,
            taffy::Line {
                start: styles::item::GridPlacement::Line(2),
                end: styles::item::GridPlacement::Span(std::num::NonZeroU16::new(3).unwrap()),
            }
        );
        assert_eq!(style.item.justify_self, Some(taffy::AlignItems::CENTER));

        assert_eq!(dom.remove_style_property(div, "grid-row"), Ok(true));
        assert_eq!(dom.remove_style_property(div, "justify-self"), Ok(true));
        let Some(Element::Div { style }) = dom.element(div) else {
            panic!("expected div element");
        };
        assert_eq!(
            style.item.grid_row,
            taffy::Line {
                start: styles::item::GridPlacement::Auto,
                end: styles::item::GridPlacement::Auto,
            }
        );
        assert_eq!(style.item.justify_self, None);
    }

    #[test]
    fn overflowing_grid_placements_never_enter_dom_state() {
        let mut dom = Dom::new();
        let div = dom.create_element(Element::Div {
            style: Box::default(),
        });
        assert_eq!(dom.set_style_property(div, "grid-row", "2"), Ok(true));
        assert_eq!(dom.set_style_property(div, "grid-column", "3"), Ok(true));
        let revision = dom.revision();

        for property in ["grid-row", "grid-column"] {
            for value in ["span", "span -1", "span 0", "span 65536", "32768"] {
                assert_eq!(
                    dom.set_style_property(div, property, value),
                    Err(StyleError::InvalidValue {
                        property: property.into(),
                        value: value.into(),
                    })
                );
                assert_eq!(dom.revision(), revision);
            }
        }

        let Some(Element::Div { style }) = dom.element(div) else {
            panic!("expected div element");
        };
        assert_eq!(
            style.item.grid_row,
            taffy::Line {
                start: styles::item::GridPlacement::Line(2),
                end: styles::item::GridPlacement::Auto,
            }
        );
        assert_eq!(
            style.item.grid_column,
            taffy::Line {
                start: styles::item::GridPlacement::Line(3),
                end: styles::item::GridPlacement::Auto,
            }
        );
    }

    #[test]
    fn style_no_ops_do_not_advance_revisions() {
        let mut dom = Dom::new();
        let div = dom.create_element(Element::Div {
            style: Box::default(),
        });
        let initial_revision = dom.revision();

        assert_eq!(dom.set_style_property(div, "flex-grow", "0.00"), Ok(false));
        assert_eq!(dom.revision(), initial_revision);

        assert_eq!(dom.set_style_property(div, "flex-grow", "1"), Ok(true));
        let changed_revision = dom.revision();
        assert!(changed_revision > initial_revision);
        assert_eq!(dom.set_style_property(div, "flex-grow", "1.0"), Ok(false));
        assert_eq!(dom.revision(), changed_revision);

        assert_eq!(dom.remove_style_property(div, "flex-grow"), Ok(true));
        let removed_revision = dom.revision();
        assert_eq!(dom.remove_style_property(div, "flex-grow"), Ok(false));
        assert_eq!(dom.revision(), removed_revision);
    }

    #[test]
    fn style_errors_distinguish_targets_properties_and_values() {
        let mut dom = Dom::new();
        let div = dom.create_element(Element::Div {
            style: Box::default(),
        });
        let text = dom.create_text("content");
        let stale = dom.create_element(Element::Div {
            style: Box::default(),
        });
        dom.remove_subtree(stale).unwrap();

        assert_eq!(
            dom.set_style_property(stale, "width", "10px"),
            Err(StyleError::NodeNotFound(stale))
        );
        assert_eq!(
            dom.set_style_property(text, "width", "10px"),
            Err(StyleError::NodeNotElement(text))
        );
        assert_eq!(
            dom.set_style_property(div, "unknown", "10px"),
            Err(StyleError::UnsupportedProperty("unknown".into()))
        );
        assert_eq!(
            dom.set_style_property(div, "width", "large"),
            Err(StyleError::InvalidValue {
                property: "width".into(),
                value: "large".into(),
            })
        );
        assert_eq!(
            dom.remove_style_property(div, "unknown"),
            Err(StyleError::UnsupportedProperty("unknown".into()))
        );
    }

    #[test]
    fn invalid_numeric_styles_never_enter_dom_state() {
        let mut dom = Dom::new();
        let flex = dom.create_element(Element::Flex {
            style: Box::default(),
        });
        let initial_revision = dom.revision();

        for (property, value) in [
            ("width", "NaNpx"),
            ("height", "infpx"),
            ("width", "-1px"),
            ("padding", "-1px"),
            ("gap", "-1%"),
            ("flex-basis", "-1px"),
            ("flex-grow", "-1"),
            ("flex-shrink", "NaN"),
        ] {
            assert_eq!(
                dom.set_style_property(flex, property, value),
                Err(StyleError::InvalidValue {
                    property: property.into(),
                    value: value.into(),
                }),
                "{property}: {value}"
            );
        }

        assert_eq!(dom.revision(), initial_revision);
        assert_eq!(
            dom.element(flex),
            Some(&Element::Flex {
                style: Box::default()
            })
        );
    }

    #[test]
    fn style_contract_accepts_auto_percent_dimensions_and_hex_colors() {
        let mut dom = Dom::new();
        let window = dom.create_element(Element::Window {
            style: Box::default(),
        });
        let div = dom.create_element(Element::Div {
            style: Box::default(),
        });

        assert_eq!(dom.set_style_property(window, "width", "50%"), Ok(true));
        assert_eq!(dom.set_style_property(window, "height", "640px"), Ok(true));
        assert_eq!(
            dom.set_style_property(window, "background-color", "#1234"),
            Ok(true)
        );
        assert_eq!(dom.set_style_property(div, "align-self", "auto"), Ok(false));
        assert_eq!(
            dom.set_style_property(div, "justify-self", "auto"),
            Ok(false)
        );
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
                item: styles::item::ItemStyle {
                    flex_grow: 1.0,
                    ..styles::item::ItemStyle::default()
                },
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
        let parent = dom.create_element(Element::Text {
            style: Box::default(),
        });
        let child = dom.create_text("content");
        dom.append_child(parent, child).unwrap();

        dom.remove_subtree(parent).unwrap();

        assert!(!dom.contains(parent));
        assert!(!dom.contains(child));
    }

    #[test]
    fn ordinary_elements_reject_text_nodes_without_mutation() {
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
            let revision = dom.revision();
            let parent_revisions = dom.node(parent).unwrap().revisions();
            let text_revisions = dom.node(text).unwrap().revisions();

            assert_eq!(
                dom.append_child(parent, text),
                Err(DomError::InvalidRelationship {
                    parent,
                    child: text,
                })
            );
            assert!(dom.children(parent).unwrap().is_empty());
            assert_eq!(dom.parent(text), None);
            assert_eq!(dom.revision(), revision);
            assert_eq!(dom.node(parent).unwrap().revisions(), parent_revisions);
            assert_eq!(dom.node(text).unwrap().revisions(), text_revisions);
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
    fn creates_default_elements_from_checked_tags() {
        let mut dom = Dom::new();
        for (name, tag) in [
            ("window", ElementTag::Window),
            ("div", ElementTag::Div),
            ("flex", ElementTag::Flex),
            ("grid", ElementTag::Grid),
            ("text", ElementTag::Text),
        ] {
            assert_eq!(ElementTag::try_from(name), Ok(tag));
            let id = dom.create_element_tag(tag);
            assert_eq!(dom.element_tag(id), Ok(tag));
            assert_eq!(dom.element(id).unwrap().local_name(), name);
        }

        let revision = dom.revision();
        assert_eq!(
            ElementTag::try_from("canvas"),
            Err(ElementTagError("canvas".into()))
        );
        assert_eq!(dom.revision(), revision);
    }

    #[test]
    fn traverses_parents_children_siblings_and_connectedness() {
        let mut dom = Dom::new();
        let window = dom.create_element_tag(ElementTag::Window);
        let parent = dom.create_element_tag(ElementTag::Div);
        let first = dom.create_element_tag(ElementTag::Div);
        let second = dom.create_element_tag(ElementTag::Text);
        dom.append_child(dom.root(), window).unwrap();
        dom.append_child(window, parent).unwrap();
        dom.append_child(parent, first).unwrap();
        dom.append_child(parent, second).unwrap();

        assert_eq!(dom.parent_node(first), Ok(Some(parent)));
        assert_eq!(dom.first_child(parent), Ok(Some(first)));
        assert_eq!(dom.last_child(parent), Ok(Some(second)));
        assert_eq!(dom.previous_sibling(first), Ok(None));
        assert_eq!(dom.next_sibling(first), Ok(Some(second)));
        assert_eq!(dom.previous_sibling(second), Ok(Some(first)));
        assert_eq!(dom.next_sibling(second), Ok(None));
        assert_eq!(dom.contains_node(parent, second), Ok(true));
        assert_eq!(dom.contains_node(second, parent), Ok(false));
        assert_eq!(dom.is_connected(second), Ok(true));

        dom.detach(parent).unwrap();
        assert_eq!(dom.is_connected(parent), Ok(false));
        assert_eq!(dom.is_connected(second), Ok(false));
    }

    #[test]
    fn insert_before_validates_references_and_handles_same_parent_moves() {
        let mut dom = Dom::new();
        let parent = dom.create_element_tag(ElementTag::Div);
        let first = dom.create_element_tag(ElementTag::Div);
        let second = dom.create_element_tag(ElementTag::Div);
        let third = dom.create_element_tag(ElementTag::Div);
        let outsider = dom.create_element_tag(ElementTag::Div);
        dom.append_child(parent, first).unwrap();
        dom.append_child(parent, second).unwrap();
        dom.append_child(parent, third).unwrap();

        dom.insert_before(parent, third, Some(first)).unwrap();
        assert_eq!(dom.children(parent), Some(&[third, first, second][..]));

        let revision = dom.revision();
        dom.insert_before(parent, first, Some(second)).unwrap();
        dom.insert_before(parent, first, Some(first)).unwrap();
        assert_eq!(dom.revision(), revision);
        assert_eq!(dom.children(parent), Some(&[third, first, second][..]));

        assert_eq!(
            dom.insert_before(parent, third, Some(outsider)),
            Err(DomError::NotAChild {
                parent,
                child: outsider,
            })
        );
        assert_eq!(dom.children(parent), Some(&[third, first, second][..]));
    }

    #[test]
    fn remove_and_replace_require_direct_children_and_leave_old_nodes_detached() {
        let mut dom = Dom::new();
        let first_window = dom.create_element_tag(ElementTag::Window);
        let second_window = dom.create_element_tag(ElementTag::Window);
        let unrelated = dom.create_element_tag(ElementTag::Window);
        dom.append_child(dom.root(), first_window).unwrap();

        dom.replace_child(dom.root(), second_window, first_window)
            .unwrap();
        assert_eq!(dom.children(dom.root()), Some(&[second_window][..]));
        assert_eq!(dom.parent(first_window), None);
        assert_eq!(dom.parent(second_window), Some(dom.root()));
        assert!(dom.contains(first_window));

        assert_eq!(
            dom.remove_child(dom.root(), unrelated),
            Err(DomError::NotAChild {
                parent: dom.root(),
                child: unrelated,
            })
        );
        dom.remove_child(dom.root(), second_window).unwrap();
        assert_eq!(dom.parent(second_window), None);
        assert!(dom.children(dom.root()).unwrap().is_empty());
    }

    #[test]
    fn text_content_concatenates_and_replaces_without_destroying_old_children() {
        let mut dom = Dom::new();
        let outer = dom.create_element_tag(ElementTag::Text);
        let before = dom.create_text("before ");
        let inner = dom.create_element_tag(ElementTag::Text);
        let nested = dom.create_text("nested");
        dom.append_child(outer, before).unwrap();
        dom.append_child(outer, inner).unwrap();
        dom.append_child(inner, nested).unwrap();

        assert_eq!(dom.text_content(outer), Ok("before nested".into()));
        assert_eq!(dom.set_text_content(outer, "after"), Ok(true));
        let replacement = dom.first_child(outer).unwrap().unwrap();
        assert_eq!(dom.text(replacement), Some("after"));
        assert_eq!(dom.parent(before), None);
        assert_eq!(dom.parent(inner), None);
        assert!(dom.contains(before));
        assert!(dom.contains(nested));
        assert_eq!(dom.set_text_content(outer, "after"), Ok(false));
        assert_eq!(dom.set_text(replacement, "updated"), Ok(true));
        assert_eq!(dom.set_text(replacement, "updated"), Ok(false));
        assert_eq!(
            dom.set_text_content(dom.root(), "invalid"),
            Err(DomError::TextContentNotSupported(dom.root()))
        );

        for tag in [
            ElementTag::Window,
            ElementTag::Div,
            ElementTag::Flex,
            ElementTag::Grid,
        ] {
            let non_text = dom.create_element_tag(tag);
            let old_child = dom.create_element_tag(ElementTag::Div);
            dom.append_child(non_text, old_child).unwrap();
            let revision = dom.revision();
            let children = dom.children(non_text).unwrap().to_vec();
            let revisions = dom.node(non_text).unwrap().revisions();

            assert_eq!(
                dom.set_text_content(non_text, "invalid"),
                Err(DomError::TextContentNotSupported(non_text))
            );
            assert_eq!(dom.children(non_text), Some(children.as_slice()));
            assert_eq!(dom.node(non_text).unwrap().revisions(), revisions);
            assert_eq!(dom.revision(), revision);
        }
    }

    #[test]
    fn reclamation_retains_connected_and_live_detached_components() {
        let mut dom = Dom::new();
        let window = dom.create_element_tag(ElementTag::Window);
        let connected = dom.create_element_tag(ElementTag::Div);
        dom.append_child(dom.root(), window).unwrap();
        dom.append_child(window, connected).unwrap();

        let detached_root = dom.create_element_tag(ElementTag::Div);
        let live_descendant = dom.create_element_tag(ElementTag::Div);
        let sibling = dom.create_element_tag(ElementTag::Text);
        dom.append_child(detached_root, live_descendant).unwrap();
        dom.append_child(detached_root, sibling).unwrap();

        let unreachable = dom.create_element_tag(ElementTag::Grid);
        let unreachable_child = dom.create_element_tag(ElementTag::Div);
        dom.append_child(unreachable, unreachable_child).unwrap();
        let before_reclaim = dom.revision();

        let report = dom.reclaim_unreachable_detached([live_descendant]).unwrap();
        assert_eq!(report.roots, vec![unreachable]);
        assert!(report.nodes.contains(&unreachable));
        assert!(report.nodes.contains(&unreachable_child));
        assert!(!report.nodes.is_empty());
        assert_eq!(dom.revision(), before_reclaim + 1);
        assert!(dom.contains(connected));
        assert!(dom.contains(detached_root));
        assert!(dom.contains(live_descendant));
        assert!(dom.contains(sibling));

        let report = dom
            .reclaim_unreachable_detached(std::iter::empty())
            .unwrap();
        assert_eq!(report.roots, vec![detached_root]);
        assert!(!dom.contains(detached_root));
        assert!(!dom.contains(live_descendant));
        assert!(!dom.contains(sibling));
        assert!(dom.contains(connected));
    }

    #[test]
    fn reclamation_rejects_stale_live_wrapper_roots_without_sweeping() {
        let mut dom = Dom::new();
        let stale = dom.create_element_tag(ElementTag::Div);
        dom.remove_subtree(stale).unwrap();
        let detached = dom.create_element_tag(ElementTag::Grid);
        let revision = dom.revision();

        assert_eq!(
            dom.reclaim_unreachable_detached([stale]),
            Err(DomError::NodeNotFound(stale))
        );
        assert!(dom.contains(detached));
        assert_eq!(dom.revision(), revision);
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
