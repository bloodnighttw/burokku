//! Uncomputed UI nodes, called elements.

use std::collections::HashMap;

use crate::ui::elements::styles::{set_style, Style};
mod error;

pub(crate) mod styles;

pub use error::DocumentError;

pub(super) const BODY_ID: u64 = 0;

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum ElementKind {
    Text(String),
    Comment(String),
    Button,
    Div,
    Heading(u8),
    Image,
    Select,
    Span,
    Body,
    Other(String),
}

impl ElementKind {
    pub fn is_element(&self) -> bool {
        !matches!(self, Self::Text(_) | Self::Comment(_))
    }
}

impl From<&str> for ElementKind {
    fn from(name: &str) -> Self {
        name.to_owned().into()
    }
}

impl From<String> for ElementKind {
    fn from(name: String) -> Self {
        let name = name.to_ascii_lowercase();
        match name.as_str() {
            "button" => Self::Button,
            "div" => Self::Div,
            "h1" => Self::Heading(1),
            "h2" => Self::Heading(2),
            "h3" => Self::Heading(3),
            "h4" => Self::Heading(4),
            "h5" => Self::Heading(5),
            "h6" => Self::Heading(6),
            "img" => Self::Image,
            "select" => Self::Select,
            "span" => Self::Span,
            _ => Self::Other(name),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    pub kind: ElementKind,
    pub parent: Option<u64>,
    pub children: Vec<u64>,
    pub style: Style,
}

#[derive(Clone, Debug)]
pub struct Document {
    nodes: HashMap<u64, Element>,
    next_id: u64,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    pub fn new() -> Self {
        let body = Element {
            kind: ElementKind::Body,
            style: Style::default(),
            children: Vec::new(),
            parent: None,
        };
        Self {
            nodes: HashMap::from([(BODY_ID, body)]),
            next_id: 1,
        }
    }

    pub fn body(&self) -> &Element {
        self.nodes
            .get(&BODY_ID)
            .expect("the document body always exists")
    }

    pub fn node(&self, id: u64) -> Result<&Element, DocumentError> {
        self.nodes.get(&id).ok_or(DocumentError::MissingNode(id))
    }

    pub fn create_node(&mut self, kind: ElementKind) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.insert(
            id,
            Element {
                kind,
                style: Style::default(),
                children: Vec::new(),
                parent: None,
            },
        );
        id
    }

    pub fn set_text(&mut self, id: u64, text: String) -> Result<(), DocumentError> {
        let node = self.node_mut(id)?;
        match &mut node.kind {
            ElementKind::Text(node_text) | ElementKind::Comment(node_text) => {
                *node_text = text;
                Ok(())
            }
            _ => Err(DocumentError::NotText(id)),
        }
    }

    pub fn set_style(
        &mut self,
        id: u64,
        name: &str,
        value: Option<&str>,
    ) -> Result<(), DocumentError> {
        let node = self.node_mut(id)?;
        if !node.kind.is_element() {
            return Err(DocumentError::NotElement(id));
        }
        set_style(&mut node.style, name, value).map_err(DocumentError::Style)
    }

    pub fn insert(
        &mut self,
        parent: u64,
        child: u64,
        before: Option<u64>,
    ) -> Result<(), DocumentError> {
        self.node(parent)?;
        self.node(child)?;
        if child == BODY_ID {
            return Err(DocumentError::MoveBody);
        }
        if before == Some(child) {
            return Ok(());
        }
        if let Some(anchor) = before {
            if self.node(anchor)?.parent != Some(parent) {
                return Err(DocumentError::MissingAnchor { parent, anchor });
            }
        }

        let mut ancestor = Some(parent);
        while let Some(id) = ancestor {
            if id == child {
                return Err(DocumentError::Cycle { parent, child });
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

    pub fn remove(&mut self, parent: u64, child: u64) -> Result<(), DocumentError> {
        let index = self
            .node(parent)?
            .children
            .iter()
            .position(|candidate| *candidate == child)
            .ok_or(DocumentError::NotAChild { parent, child })?;
        self.node_mut(parent)?.children.remove(index);
        self.node_mut(child)?.parent = None;
        Ok(())
    }

    fn node_mut(&mut self, id: u64) -> Result<&mut Element, DocumentError> {
        self.nodes
            .get_mut(&id)
            .ok_or(DocumentError::MissingNode(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nodes_can_be_moved_detached_and_reattached() {
        let mut document = Document::new();
        let first = document.create_node(ElementKind::Div);
        let second = document.create_node(ElementKind::Span);
        let text = document.create_node(ElementKind::Text("hello".into()));
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
        let parent = document.create_node(ElementKind::Div);
        let child = document.create_node(ElementKind::Div);
        document.insert(BODY_ID, parent, None).unwrap();
        document.insert(parent, child, None).unwrap();

        assert!(matches!(
            document.insert(child, parent, None),
            Err(DocumentError::Cycle { .. })
        ));
    }

    #[test]
    fn element_names_map_to_semantic_kinds() {
        assert_eq!(ElementKind::from("BUTTON".to_owned()), ElementKind::Button);
        assert_eq!(ElementKind::from("h3".to_owned()), ElementKind::Heading(3));
        assert_eq!(
            ElementKind::from("CUSTOM-CARD".to_owned()),
            ElementKind::Other("custom-card".to_owned())
        );
    }
}
