use std::collections::HashMap;

use thiserror::Error;

use super::style::{set_style, Style};

pub const BODY_ID: u64 = 0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Element(String),
    Text,
    Comment,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub kind: NodeKind,
    pub text: String,
    pub style: Style,
    pub children: Vec<u64>,
    pub parent: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct Document {
    nodes: HashMap<u64, Node>,
    next_id: u64,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    pub fn new() -> Self {
        let body = Node {
            kind: NodeKind::Element("body".into()),
            text: String::new(),
            style: Style::default(),
            children: Vec::new(),
            parent: None,
        };
        Self {
            nodes: HashMap::from([(BODY_ID, body)]),
            next_id: 1,
        }
    }

    pub fn body(&self) -> &Node {
        self.nodes
            .get(&BODY_ID)
            .expect("the document body always exists")
    }

    pub fn node(&self, id: u64) -> Result<&Node, DomError> {
        self.nodes.get(&id).ok_or(DomError::MissingNode(id))
    }

    pub fn create_node(&mut self, kind: NodeKind) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.insert(
            id,
            Node {
                kind,
                text: String::new(),
                style: Style::default(),
                children: Vec::new(),
                parent: None,
            },
        );
        id
    }

    pub fn set_text(&mut self, id: u64, text: String) -> Result<(), DomError> {
        let node = self.node_mut(id)?;
        if !matches!(node.kind, NodeKind::Text | NodeKind::Comment) {
            return Err(DomError::NotText(id));
        }
        node.text = text;
        Ok(())
    }

    pub fn set_style(&mut self, id: u64, name: &str, value: Option<&str>) -> Result<(), DomError> {
        let node = self.node_mut(id)?;
        if !matches!(node.kind, NodeKind::Element(_)) {
            return Err(DomError::NotElement(id));
        }
        set_style(&mut node.style, name, value).map_err(DomError::Style)
    }

    pub fn insert(&mut self, parent: u64, child: u64, before: Option<u64>) -> Result<(), DomError> {
        self.node(parent)?;
        self.node(child)?;
        if child == BODY_ID {
            return Err(DomError::MoveBody);
        }
        if before == Some(child) {
            return Ok(());
        }
        if let Some(anchor) = before {
            if self.node(anchor)?.parent != Some(parent) {
                return Err(DomError::MissingAnchor { parent, anchor });
            }
        }

        let mut ancestor = Some(parent);
        while let Some(id) = ancestor {
            if id == child {
                return Err(DomError::Cycle { parent, child });
            }
            ancestor = self.node(id)?.parent;
        }

        if let Some(old_parent) = self.node(child)?.parent {
            let old_children = &mut self.node_mut(old_parent)?.children;
            if let Some(index) = old_children
                .iter()
                .position(|candidate| *candidate == child)
            {
                old_children.remove(index);
            }
        }

        let index = match before {
            Some(anchor) => self
                .node(parent)?
                .children
                .iter()
                .position(|candidate| *candidate == anchor)
                .expect("the anchor was validated before moving the child"),
            None => self.node(parent)?.children.len(),
        };
        self.node_mut(parent)?.children.insert(index, child);
        self.node_mut(child)?.parent = Some(parent);
        Ok(())
    }

    pub fn remove(&mut self, parent: u64, child: u64) -> Result<(), DomError> {
        let index = self
            .node(parent)?
            .children
            .iter()
            .position(|candidate| *candidate == child)
            .ok_or(DomError::NotAChild { parent, child })?;
        self.node_mut(parent)?.children.remove(index);
        self.node_mut(child)?.parent = None;
        Ok(())
    }

    fn node_mut(&mut self, id: u64) -> Result<&mut Node, DomError> {
        self.nodes.get_mut(&id).ok_or(DomError::MissingNode(id))
    }
}

#[derive(Debug, Error)]
pub enum DomError {
    #[error("DOM node {0} does not exist")]
    MissingNode(u64),
    #[error("DOM node {0} is not an element")]
    NotElement(u64),
    #[error("DOM node {0} is not a text node")]
    NotText(u64),
    #[error("document.body cannot be moved")]
    MoveBody,
    #[error("inserting node {child} below {parent} would create a cycle")]
    Cycle { parent: u64, child: u64 },
    #[error("node {anchor} is not a child of {parent}")]
    MissingAnchor { parent: u64, anchor: u64 },
    #[error("node {child} is not a child of {parent}")]
    NotAChild { parent: u64, child: u64 },
    #[error(transparent)]
    Style(#[from] super::style::StyleError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nodes_can_be_moved_detached_and_reattached() {
        let mut document = Document::new();
        let first = document.create_node(NodeKind::Element("div".into()));
        let second = document.create_node(NodeKind::Element("span".into()));
        let text = document.create_node(NodeKind::Text);
        document.set_text(text, "hello".into()).unwrap();
        document.insert(BODY_ID, first, None).unwrap();
        document.insert(BODY_ID, second, None).unwrap();
        document.insert(first, text, None).unwrap();

        document.insert(second, text, None).unwrap();
        assert!(document.node(first).unwrap().children.is_empty());
        assert_eq!(document.node(second).unwrap().children, [text]);

        document.remove(second, text).unwrap();
        assert_eq!(document.node(text).unwrap().parent, None);
        document.insert(first, text, None).unwrap();
        assert_eq!(document.node(text).unwrap().parent, Some(first));
    }

    #[test]
    fn rejects_cycles() {
        let mut document = Document::new();
        let parent = document.create_node(NodeKind::Element("div".into()));
        let child = document.create_node(NodeKind::Element("div".into()));
        document.insert(BODY_ID, parent, None).unwrap();
        document.insert(parent, child, None).unwrap();

        assert!(matches!(
            document.insert(child, parent, None),
            Err(DomError::Cycle { .. })
        ));
    }
}
