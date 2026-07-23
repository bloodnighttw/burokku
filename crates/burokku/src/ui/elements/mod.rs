//! This module contain the ** uncomputed ** node of ui, which is called as
//! **Elements**

use std::collections::HashMap;

use crate::ui::elements::{
    error::DocumentError,
    styles::{set_style, Style},
};
mod error;

pub(crate) mod styles;

const BODY_ID: u64 = 0;

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum ElementKind {
    Text(String),
    Div,
    Span,
    Body,
    // TODO:
    // Image,
    // Button,
    // Select,
    // Option,
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
        let ElementKind::Text(node_text) = &mut node.kind else {
            return Err(DocumentError::NotText(id));
        };
        *node_text = text;
        Ok(())
    }

    pub fn set_style(
        &mut self,
        id: u64,
        name: &str,
        value: Option<&str>,
    ) -> Result<(), DocumentError> {
        let node = self.node_mut(id)?;
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
