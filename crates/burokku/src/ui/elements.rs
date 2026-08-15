use self::styles::{flex::FlexStyle, grid::GridStyle};
use slotmap::{new_key_type, SlotMap};
use thiserror::Error;

mod iter;

pub use iter::ElementsIter;
pub mod styles;

new_key_type! {
    /// A stable, generation-checked handle to an element in a [`Dom`].
    ///
    /// The handle remains valid when the arena grows, when the element moves in
    /// the tree, and in cloned DOM snapshots. Once its element is removed, the
    /// generation prevents the handle from referring to a later allocation that
    /// reuses the same slot.
    pub struct NodeId;
}

/// The data that determines an element's type and content.
///
/// Parent and child relationships deliberately live in [`Node`] rather than in
/// this enum. This keeps elements in an arena, so callers retain [`NodeId`]
/// handles instead of references into a recursively owned tree.
#[derive(Clone, Debug, PartialEq)]
pub enum Elements {
    // for <app> tag
    App,
    // for <window> tag
    Window,
    // for <div> tag
    Div,
    // for <flex> tag
    Flex {
        style: Box<FlexStyle>,
    },
    // for <grid> tag
    Grid {
        style: Box<GridStyle>,
    },
    // for <text> tag
    Text,
    /// Internal element used for text content. It is not intended to be
    /// constructed directly by application code.
    _String {
        string: String,
    },
}

/// Returns true if `child` is a valid child of `self`.
impl Elements {
    fn accepts(&self, child: &Self) -> bool {
        match self {
            Self::App => matches!(child, Self::Window),
            Self::Window | Self::Div | Self::Flex { .. } | Self::Grid { .. } => matches!(
                child,
                Self::Div | Self::Flex { .. } | Self::Grid { .. } | Self::Text
            ),
            Self::Text => matches!(child, Self::Text | Self::_String { .. }),
            Self::_String { .. } => false,
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

/// An arena entry containing an element and its tree relationships.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    element: Elements,
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    revisions: NodeRevisions,
}

impl Node {
    pub fn element(&self) -> &Elements {
        &self.element
    }

    pub fn parent(&self) -> Option<NodeId> {
        self.parent
    }

    pub fn children(&self) -> &[NodeId] {
        &self.children
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
    nodes: SlotMap<NodeId, Node>,
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
        let root = nodes.insert(Node {
            element: Elements::App,
            parent: None,
            children: Vec::new(),
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

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id)
    }

    pub fn element(&self, id: NodeId) -> Option<&Elements> {
        self.node(id).map(Node::element)
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).and_then(Node::parent)
    }

    pub fn children(&self, id: NodeId) -> Option<&[NodeId]> {
        self.node(id).map(Node::children)
    }

    /// Allocates a detached element and returns its stable handle.
    pub fn create(&mut self, element: Elements) -> NodeId {
        let id = self.nodes.insert(Node {
            element,
            parent: None,
            children: Vec::new(),
            revisions: NodeRevisions::default(),
        });
        self.bump_revision();
        id
    }

    /// Replaces an element's data without changing its stable handle.
    ///
    /// The replacement must remain valid for both its parent and its existing
    /// children. The root must always remain an `App`.
    pub fn set_element(&mut self, id: NodeId, element: Elements) -> Result<(), DomError> {
        let node = self.node(id).ok_or(DomError::NodeNotFound(id))?;

        if id == self.root && !matches!(element, Elements::App) {
            return Err(DomError::RootMustBeApp);
        }
        if id != self.root && matches!(element, Elements::App) {
            return Err(DomError::AppMustBeRoot);
        }
        if let Some(parent) = node.parent {
            let parent_element = &self.nodes[parent].element;
            if !parent_element.accepts(&element) {
                return Err(DomError::InvalidRelationship { parent, child: id });
            }
        }
        if node
            .children
            .iter()
            .any(|child| !element.accepts(&self.nodes[*child].element))
        {
            return Err(DomError::InvalidChildren(id));
        }
        if matches!(element, Elements::App) && node.children.len() > 1 {
            return Err(DomError::AppAlreadyHasWindow);
        }

        let revision_kind = ElementRevisionKind::between(&node.element, &element);
        if revision_kind == ElementRevisionKind::None {
            return Ok(());
        }

        let node = &mut self.nodes[id];
        node.element = element;
        match revision_kind {
            ElementRevisionKind::None => unreachable!("no-op replacements return above"),
            ElementRevisionKind::Style => bump(&mut node.revisions.style),
            ElementRevisionKind::Content => bump(&mut node.revisions.content),
            ElementRevisionKind::All => {
                bump(&mut node.revisions.structure);
                bump(&mut node.revisions.style);
                bump(&mut node.revisions.content);
            }
        }
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

        if child == self.root || matches!(child_node.element, Elements::App) {
            return Err(DomError::AppMustBeRoot);
        }
        if !parent_node.element.accepts(&child_node.element) {
            return Err(DomError::InvalidRelationship { parent, child });
        }
        if self.is_ancestor_or_self(child, parent) {
            return Err(DomError::Cycle { parent, child });
        }

        let same_parent = child_node.parent == Some(parent);
        let final_len = parent_node.children.len() - usize::from(same_parent);
        if index > final_len {
            return Err(DomError::IndexOutOfBounds {
                parent,
                index,
                len: final_len,
            });
        }
        if matches!(parent_node.element, Elements::App)
            && parent_node
                .children
                .iter()
                .any(|existing| *existing != child)
        {
            return Err(DomError::AppAlreadyHasWindow);
        }

        let old_parent = child_node.parent;
        if let Some(old_parent) = old_parent {
            self.nodes[old_parent]
                .children
                .retain(|existing| *existing != child);
            bump(&mut self.nodes[old_parent].revisions.structure);
        }
        self.nodes[parent].children.insert(index, child);
        if old_parent != Some(parent) {
            self.nodes[child].parent = Some(parent);
            bump(&mut self.nodes[child].revisions.structure);
        }
        if old_parent != Some(parent) {
            bump(&mut self.nodes[parent].revisions.structure);
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
            self.nodes[parent].children.retain(|child| *child != id);
            bump(&mut self.nodes[parent].revisions.structure);
            self.nodes[id].parent = None;
            bump(&mut self.nodes[id].revisions.structure);
            self.bump_revision();
        }
        Ok(())
    }

    /// Removes a node and all descendants, invalidating all of their handles.
    pub fn remove_subtree(&mut self, id: NodeId) -> Result<Elements, DomError> {
        if id == self.root {
            return Err(DomError::CannotRemoveRoot);
        }
        let parent = self.node(id).ok_or(DomError::NodeNotFound(id))?.parent;
        if let Some(parent) = parent {
            self.nodes[parent].children.retain(|child| *child != id);
            bump(&mut self.nodes[parent].revisions.structure);
        }

        let removed = self
            .nodes
            .remove(id)
            .expect("the node was checked immediately before removal");
        let mut pending = removed.children;
        while let Some(descendant) = pending.pop() {
            if let Some(node) = self.nodes.remove(descendant) {
                pending.extend(node.children);
            }
        }
        self.bump_revision();
        Ok(removed.element)
    }

    /// Iterates over the reachable tree in pre-order, yielding stable IDs.
    pub fn iter(&self) -> ElementsIter<'_> {
        ElementsIter::new(self)
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ElementRevisionKind {
    None,
    Style,
    Content,
    All,
}

impl ElementRevisionKind {
    fn between(current: &Elements, replacement: &Elements) -> Self {
        if current == replacement {
            return Self::None;
        }

        match (current, replacement) {
            (Elements::Flex { .. }, Elements::Flex { .. })
            | (Elements::Grid { .. }, Elements::Grid { .. }) => Self::Style,
            (Elements::_String { .. }, Elements::_String { .. }) => Self::Content,
            _ => Self::All,
        }
    }
}

fn bump(revision: &mut u64) {
    *revision = revision
        .checked_add(1)
        .expect("DOM revision counter overflowed");
}

impl<'a> IntoIterator for &'a Dom {
    type Item = (NodeId, &'a Elements);
    type IntoIter = ElementsIter<'a>;

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
    #[error("the root element must remain an App")]
    RootMustBeApp,
    #[error("cannot insert node {child:?} under node {parent:?}")]
    InvalidRelationship { parent: NodeId, child: NodeId },
    #[error("node {0:?}'s existing children are invalid for its new element type")]
    InvalidChildren(NodeId),
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
        let window = dom.create(Elements::Window);
        let first_parent = dom.create(Elements::Div);
        let second_parent = dom.create(Elements::Div);
        let child = dom.create(Elements::Text);

        dom.append_child(dom.root(), window).unwrap();
        dom.append_child(window, first_parent).unwrap();
        dom.append_child(window, second_parent).unwrap();
        dom.append_child(first_parent, child).unwrap();
        dom.append_child(second_parent, child).unwrap();
        dom.set_element(child, Elements::Text).unwrap();

        assert_eq!(dom.parent(child), Some(second_parent));
        assert!(matches!(dom.element(child), Some(Elements::Text)));

        let snapshot = dom.clone();
        assert!(matches!(snapshot.element(child), Some(Elements::Text)));
        assert_eq!(snapshot.parent(child), Some(second_parent));
    }

    #[test]
    fn removed_handles_never_refer_to_reused_slots() {
        let mut dom = Dom::new();
        let stale = dom.create(Elements::Div);
        dom.remove_subtree(stale).unwrap();
        let replacement = dom.create(Elements::Div);

        assert_ne!(stale, replacement);
        assert!(!dom.contains(stale));
        assert!(dom.contains(replacement));
        assert_eq!(
            dom.set_element(stale, Elements::Div),
            Err(DomError::NodeNotFound(stale))
        );
    }

    #[test]
    fn invalid_mutations_leave_the_tree_unchanged() {
        let mut dom = Dom::new();
        let window = dom.create(Elements::Window);
        let div = dom.create(Elements::Div);
        let text = dom.create(Elements::Text);
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
    fn tracks_style_and_content_revisions_independently() {
        let mut dom = Dom::new();
        let flex = dom.create(Elements::Flex {
            style: Box::default(),
        });
        let string = dom.create(Elements::_String {
            string: "before".into(),
        });

        let style = FlexStyle {
            grow: 1.0,
            ..FlexStyle::default()
        };
        dom.set_element(
            flex,
            Elements::Flex {
                style: Box::new(style),
            },
        )
        .unwrap();
        dom.set_element(
            string,
            Elements::_String {
                string: "after".into(),
            },
        )
        .unwrap();

        assert_eq!(
            dom.node(flex).unwrap().revisions(),
            NodeRevisions {
                style: 1,
                ..NodeRevisions::default()
            }
        );
        assert_eq!(
            dom.node(string).unwrap().revisions(),
            NodeRevisions {
                content: 1,
                ..NodeRevisions::default()
            }
        );
    }

    #[test]
    fn structural_mutations_mark_only_affected_nodes() {
        let mut dom = Dom::new();
        let window = dom.create(Elements::Window);
        let first = dom.create(Elements::Div);
        let second = dom.create(Elements::Div);
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
        let div = dom.create(Elements::Div);
        let dom_revision = dom.revision();
        let node_revisions = dom.node(div).unwrap().revisions();

        dom.set_element(div, Elements::Div).unwrap();

        assert_eq!(dom.revision(), dom_revision);
        assert_eq!(dom.node(div).unwrap().revisions(), node_revisions);
    }

    #[test]
    fn removing_a_subtree_invalidates_descendants() {
        let mut dom = Dom::new();
        let parent = dom.create(Elements::Div);
        let child = dom.create(Elements::Text);
        dom.append_child(parent, child).unwrap();

        dom.remove_subtree(parent).unwrap();

        assert!(!dom.contains(parent));
        assert!(!dom.contains(child));
    }
}
