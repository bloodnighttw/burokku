use std::collections::{HashMap, HashSet};

use slotmap::Key;
#[cfg(test)]
use slotmap::KeyData;

use crate::ui::elements::NodeId as DomNodeId;

use super::error::LayoutError;

/// Private identity of one generated layout box.
///
/// The initial representation has one box per participating DOM element, so it
/// can preserve the complete generation-checked DOM key. Keeping this wrapper
/// private lets a future generated-box arena replace that encoding without
/// leaking Taffy IDs to rendering or event APIs.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct LayoutId(taffy::NodeId);

impl LayoutId {
    pub(super) fn for_dom(id: DomNodeId) -> Self {
        Self(taffy::NodeId::new(id.data().as_ffi()))
    }

    pub(super) const fn from_taffy(id: taffy::NodeId) -> Self {
        Self(id)
    }

    pub(super) const fn into_taffy(self) -> taffy::NodeId {
        self.0
    }

    #[cfg(test)]
    pub(super) fn decode_dom(self) -> DomNodeId {
        DomNodeId::from(KeyData::from_ffi(u64::from(self.0)))
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct PositioningMeta {
    pub(super) dom_parent: Option<DomNodeId>,
    pub(super) containing_block: Option<LayoutId>,
    pub(super) source_order: usize,
}

/// A validated, revision-scoped layout relationship graph.
///
/// This is derived from a committed DOM snapshot. It is not observable DOM
/// state and may later differ from DOM parentage for positioned boxes.
#[derive(Clone, Debug, Default)]
pub(super) struct LayoutTopology {
    root: Option<LayoutId>,
    parent: HashMap<LayoutId, LayoutId>,
    children: HashMap<LayoutId, Vec<LayoutId>>,
    dom_to_layout: HashMap<DomNodeId, LayoutId>,
    layout_to_dom: HashMap<LayoutId, DomNodeId>,
    positioning: HashMap<LayoutId, PositioningMeta>,
}

impl LayoutTopology {
    pub(super) fn insert_root(&mut self, dom_id: DomNodeId) -> Result<LayoutId, LayoutError> {
        let id = self.insert_mapping(dom_id)?;
        if self.root.replace(id).is_some() {
            return Err(LayoutError::MultipleLayoutParents(id));
        }
        self.positioning.insert(
            id,
            PositioningMeta {
                dom_parent: None,
                containing_block: None,
                source_order: 0,
            },
        );
        Ok(id)
    }

    pub(super) fn insert_child(
        &mut self,
        dom_id: DomNodeId,
        dom_parent: DomNodeId,
        layout_parent: LayoutId,
        source_order: usize,
    ) -> Result<LayoutId, LayoutError> {
        if !self.children.contains_key(&layout_parent) {
            return Err(LayoutError::MissingLayoutNode(layout_parent));
        }

        let id = self.insert_mapping(dom_id)?;
        if self.parent.insert(id, layout_parent).is_some() {
            return Err(LayoutError::MultipleLayoutParents(id));
        }
        self.children
            .get_mut(&layout_parent)
            .expect("the effective parent was checked above")
            .push(id);
        self.positioning.insert(
            id,
            PositioningMeta {
                dom_parent: Some(dom_parent),
                containing_block: Some(layout_parent),
                source_order,
            },
        );
        Ok(id)
    }

    fn insert_mapping(&mut self, dom_id: DomNodeId) -> Result<LayoutId, LayoutError> {
        if self.dom_to_layout.contains_key(&dom_id) {
            return Err(LayoutError::DuplicateDomNode(dom_id));
        }

        let id = LayoutId::for_dom(dom_id);
        if self.layout_to_dom.insert(id, dom_id).is_some() {
            return Err(LayoutError::DuplicateDomNode(dom_id));
        }
        self.dom_to_layout.insert(dom_id, id);
        self.children.insert(id, Vec::new());
        Ok(id)
    }

    pub(super) fn root(&self) -> Option<LayoutId> {
        self.root
    }

    pub(super) fn parent(&self, id: LayoutId) -> Option<LayoutId> {
        self.parent.get(&id).copied()
    }

    pub(super) fn children(&self, id: LayoutId) -> Option<&[LayoutId]> {
        self.children.get(&id).map(Vec::as_slice)
    }

    pub(super) fn dom_id(&self, id: LayoutId) -> Option<DomNodeId> {
        self.layout_to_dom.get(&id).copied()
    }

    pub(super) fn layout_id(&self, id: DomNodeId) -> Option<LayoutId> {
        self.dom_to_layout.get(&id).copied()
    }

    pub(super) fn positioning(&self, id: LayoutId) -> Option<PositioningMeta> {
        self.positioning.get(&id).copied()
    }

    pub(super) fn len(&self) -> usize {
        self.children.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    pub(super) fn validate(
        &self,
        sidecar_ids: &HashSet<LayoutId>,
        max_depth: usize,
    ) -> Result<(), LayoutError> {
        let Some(root) = self.root else {
            if let Some(id) = self.children.keys().next().copied() {
                return Err(LayoutError::UnreachableLayoutNode(id));
            }
            return Ok(());
        };

        if self.parent.contains_key(&root) {
            return Err(LayoutError::MultipleLayoutParents(root));
        }
        if self.dom_to_layout.len() != self.children.len()
            || self.layout_to_dom.len() != self.children.len()
            || self.positioning.len() != self.children.len()
        {
            return Err(LayoutError::MissingLayoutNode(root));
        }

        for (&id, children) in &self.children {
            if !self.layout_to_dom.contains_key(&id) {
                return Err(LayoutError::MissingLayoutNode(id));
            }
            if !sidecar_ids.contains(&id) {
                return Err(LayoutError::MissingLayoutSidecar(id));
            }
            if id != root && !self.parent.contains_key(&id) {
                return Err(LayoutError::UnreachableLayoutNode(id));
            }

            let mut direct = HashSet::with_capacity(children.len());
            for &child in children {
                if !direct.insert(child) || self.parent.get(&child) != Some(&id) {
                    return Err(LayoutError::MultipleLayoutParents(child));
                }
                if !self.children.contains_key(&child) {
                    return Err(LayoutError::MissingLayoutNode(child));
                }
            }
        }

        for (&dom_id, &layout_id) in &self.dom_to_layout {
            if self.layout_to_dom.get(&layout_id) != Some(&dom_id) {
                return Err(LayoutError::MissingLayoutNode(layout_id));
            }
        }

        let mut visited = HashSet::with_capacity(self.len());
        let mut pending = vec![(root, 0usize)];
        while let Some((id, depth)) = pending.pop() {
            if !visited.insert(id) {
                return Err(LayoutError::LayoutCycle(id));
            }
            let dom_id = self.dom_id(id).ok_or(LayoutError::MissingLayoutNode(id))?;
            assert!(
                depth <= max_depth,
                "layout tree exceeds the supported depth of {max_depth} at node {dom_id:?}"
            );
            let children = self
                .children(id)
                .ok_or(LayoutError::MissingLayoutNode(id))?;
            pending.extend(children.iter().rev().map(|child| (*child, depth + 1)));
        }

        if let Some(id) = self
            .children
            .keys()
            .find(|id| !visited.contains(id))
            .copied()
        {
            return Err(LayoutError::UnreachableLayoutNode(id));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::ui::elements::{Dom, Element, ElementTag};

    use super::*;

    #[test]
    fn layout_ids_preserve_dom_generations_and_round_trip() {
        let mut dom = Dom::new();
        let first = dom.create_element(Element::from_tag(ElementTag::Div));
        let first_layout = LayoutId::for_dom(first);
        dom.remove_subtree(first).unwrap();
        let replacement = dom.create_element(Element::from_tag(ElementTag::Div));
        let replacement_layout = LayoutId::for_dom(replacement);

        assert_eq!(first_layout.decode_dom(), first);
        assert_eq!(replacement_layout.decode_dom(), replacement);
        assert_ne!(first_layout, replacement_layout);
    }

    #[test]
    fn topology_validates_forward_reverse_and_ordered_parent_edges() {
        let mut dom = Dom::new();
        let window = dom.create_element(Element::from_tag(ElementTag::Window));
        let first = dom.create_element(Element::from_tag(ElementTag::Div));
        let second = dom.create_element(Element::from_tag(ElementTag::Grid));
        let mut topology = LayoutTopology::default();
        let root = topology.insert_root(window).unwrap();
        let first_id = topology.insert_child(first, window, root, 0).unwrap();
        let second_id = topology.insert_child(second, window, root, 1).unwrap();
        let sidecars = HashSet::from([root, first_id, second_id]);

        topology.validate(&sidecars, 8).unwrap();

        assert_eq!(topology.layout_id(first), Some(first_id));
        assert_eq!(topology.dom_id(second_id), Some(second));
        assert_eq!(topology.parent(first_id), Some(root));
        assert_eq!(topology.children(root), Some(&[first_id, second_id][..]));
        assert_eq!(
            topology.positioning(second_id).unwrap().dom_parent,
            Some(window)
        );
        assert_eq!(
            topology.positioning(second_id).unwrap().containing_block,
            Some(root)
        );
        assert_eq!(topology.positioning(second_id).unwrap().source_order, 1);
        assert!(!topology.is_empty());
    }
}
