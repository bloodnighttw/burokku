use std::collections::HashMap;

use thiserror::Error;

pub const ROOT_ID: u64 = 0;

#[derive(Clone, Debug)]
pub struct UiDocument {
    pub commit_id: u64,
    nodes: HashMap<u64, UiNode>,
}

impl Default for UiDocument {
    fn default() -> Self {
        Self::new()
    }
}

impl UiDocument {
    pub fn new() -> Self {
        let mut nodes = HashMap::new();
        nodes.insert(
            ROOT_ID,
            UiNode {
                id: ROOT_ID,
                kind: ElementKind::Div,
                style: UiStyle {
                    display: Some(Display::Flex),
                    flex_direction: Some(FlexDirection::Column),
                    ..UiStyle::default()
                },
                text: String::new(),
                children: Vec::new(),
                parent: None,
            },
        );
        Self {
            commit_id: 0,
            nodes,
        }
    }

    pub fn root(&self) -> &UiNode {
        self.nodes
            .get(&ROOT_ID)
            .expect("the synthetic UI root always exists")
    }

    pub fn node(&self, id: u64) -> Result<&UiNode, UiMutationError> {
        self.nodes.get(&id).ok_or(UiMutationError::MissingNode(id))
    }

    pub fn apply(&mut self, update: UiUpdate) -> Result<bool, UiMutationError> {
        match update {
            UiUpdate::Mutation(mutation) => {
                self.apply_mutation(mutation)?;
                Ok(false)
            }
            UiUpdate::Flush(commit_id) => {
                self.commit_id = commit_id;
                Ok(true)
            }
        }
    }

    pub fn apply_mutation(&mut self, mutation: UiMutation) -> Result<(), UiMutationError> {
        match mutation {
            UiMutation::Create { id, kind } => {
                if id == ROOT_ID || self.nodes.contains_key(&id) {
                    return Err(UiMutationError::DuplicateNode(id));
                }
                self.nodes.insert(
                    id,
                    UiNode {
                        id,
                        kind,
                        style: UiStyle::default(),
                        text: String::new(),
                        children: Vec::new(),
                        parent: None,
                    },
                );
            }
            UiMutation::SetText { id, text } => self.node_mut(id)?.text = text,
            UiMutation::SetStyle { id, name, value } => {
                set_style(&mut self.node_mut(id)?.style, &name, value)?;
            }
            UiMutation::ClearStyle { id, name } => {
                clear_style(&mut self.node_mut(id)?.style, &name)?;
            }
            UiMutation::Insert {
                parent,
                child,
                before,
            } => self.insert(parent, child, before)?,
            UiMutation::Remove { parent, child } => self.remove(parent, child)?,
        }
        Ok(())
    }

    fn node_mut(&mut self, id: u64) -> Result<&mut UiNode, UiMutationError> {
        self.nodes
            .get_mut(&id)
            .ok_or(UiMutationError::MissingNode(id))
    }

    fn insert(
        &mut self,
        parent: u64,
        child: u64,
        before: Option<u64>,
    ) -> Result<(), UiMutationError> {
        self.node(parent)?;
        self.node(child)?;
        if child == ROOT_ID {
            return Err(UiMutationError::MoveRoot);
        }
        if before == Some(child) {
            return Ok(());
        }
        if let Some(anchor) = before {
            if self.nodes.get(&anchor).and_then(|node| node.parent) != Some(parent) {
                return Err(UiMutationError::MissingAnchor { parent, anchor });
            }
        }

        let mut ancestor = Some(parent);
        while let Some(id) = ancestor {
            if id == child {
                return Err(UiMutationError::Cycle { parent, child });
            }
            ancestor = self.node(id)?.parent;
        }

        if let Some(old_parent) = self.node(child)?.parent {
            let old_children = &mut self.node_mut(old_parent)?.children;
            if let Some(index) = old_children.iter().position(|id| *id == child) {
                old_children.remove(index);
            }
        }

        let index = match before {
            Some(anchor) => self
                .node(parent)?
                .children
                .iter()
                .position(|id| *id == anchor)
                .expect("the insertion anchor was validated before detaching the child"),
            None => self.node(parent)?.children.len(),
        };
        self.node_mut(parent)?.children.insert(index, child);
        self.node_mut(child)?.parent = Some(parent);
        Ok(())
    }

    fn remove(&mut self, parent: u64, child: u64) -> Result<(), UiMutationError> {
        let index = self
            .node(parent)?
            .children
            .iter()
            .position(|id| *id == child)
            .ok_or(UiMutationError::NotAChild { parent, child })?;
        self.node_mut(parent)?.children.remove(index);
        self.remove_subtree(child);
        Ok(())
    }

    fn remove_subtree(&mut self, id: u64) {
        let node = self
            .nodes
            .remove(&id)
            .expect("the removed subtree was validated before deletion");
        for child in node.children {
            self.remove_subtree(child);
        }
    }
}

#[derive(Clone, Debug)]
pub struct UiNode {
    pub id: u64,
    pub kind: ElementKind,
    pub style: UiStyle,
    pub text: String,
    pub children: Vec<u64>,
    parent: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElementKind {
    Div,
    Button,
    Span,
    Text,
}

impl ElementKind {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "div" => Some(Self::Div),
            "button" => Some(Self::Button),
            "span" => Some(Self::Span),
            "text" => Some(Self::Text),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub enum UiUpdate {
    Mutation(UiMutation),
    Flush(u64),
}

#[derive(Clone, Debug)]
pub enum UiMutation {
    Create {
        id: u64,
        kind: ElementKind,
    },
    SetText {
        id: u64,
        text: String,
    },
    SetStyle {
        id: u64,
        name: String,
        value: UiStyleValue,
    },
    ClearStyle {
        id: u64,
        name: String,
    },
    Insert {
        parent: u64,
        child: u64,
        before: Option<u64>,
    },
    Remove {
        parent: u64,
        child: u64,
    },
}

#[derive(Clone, Debug)]
pub enum UiStyleValue {
    Number(f32),
    String(String),
    Color([u8; 4]),
}

#[derive(Clone, Debug, Default)]
pub struct UiStyle {
    pub display: Option<Display>,
    pub flex_direction: Option<FlexDirection>,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub min_width: Option<f32>,
    pub min_height: Option<f32>,
    pub max_width: Option<f32>,
    pub max_height: Option<f32>,
    pub flex_grow: Option<f32>,
    pub flex_shrink: Option<f32>,
    pub gap: Option<f32>,
    pub padding: Option<f32>,
    pub margin: Option<f32>,
    pub background_color: Option<[u8; 4]>,
    pub color: Option<[u8; 4]>,
    pub border_color: Option<[u8; 4]>,
    pub border_width: Option<f32>,
    pub border_radius: Option<f32>,
    pub outline_color: Option<[u8; 4]>,
    pub outline_width: Option<f32>,
    pub outline_offset: Option<f32>,
    pub font_size: Option<f32>,
    pub line_height: Option<f32>,
    pub font_weight: Option<u16>,
    pub font_family: Option<String>,
}

#[derive(Clone, Copy, Debug)]
pub enum Display {
    Block,
    Flex,
    None,
}

#[derive(Clone, Copy, Debug)]
pub enum FlexDirection {
    Row,
    Column,
}

#[derive(Debug, Error)]
pub enum UiMutationError {
    #[error("UI node {0} does not exist")]
    MissingNode(u64),
    #[error("UI node {0} already exists")]
    DuplicateNode(u64),
    #[error("the synthetic UI root cannot be moved")]
    MoveRoot,
    #[error("inserting node {child} below {parent} would create a cycle")]
    Cycle { parent: u64, child: u64 },
    #[error("node {anchor} is not a child of {parent} and cannot be used as an anchor")]
    MissingAnchor { parent: u64, anchor: u64 },
    #[error("node {child} is not a child of {parent}")]
    NotAChild { parent: u64, child: u64 },
    #[error("unsupported style property '{0}'")]
    UnsupportedStyle(String),
    #[error("invalid value for style property '{0}'")]
    InvalidStyleValue(String),
}

fn set_style(style: &mut UiStyle, name: &str, value: UiStyleValue) -> Result<(), UiMutationError> {
    macro_rules! number {
        ($field:ident) => {
            if let UiStyleValue::Number(value) = value {
                style.$field = Some(value);
                return Ok(());
            }
        };
    }
    macro_rules! color {
        ($field:ident) => {
            if let UiStyleValue::Color(value) = value {
                style.$field = Some(value);
                return Ok(());
            }
        };
    }

    match name {
        "width" => number!(width),
        "height" => number!(height),
        "minWidth" => number!(min_width),
        "minHeight" => number!(min_height),
        "maxWidth" => number!(max_width),
        "maxHeight" => number!(max_height),
        "flexGrow" => number!(flex_grow),
        "flexShrink" => number!(flex_shrink),
        "gap" => number!(gap),
        "padding" => number!(padding),
        "margin" => number!(margin),
        "borderWidth" => number!(border_width),
        "borderRadius" => number!(border_radius),
        "outlineWidth" => number!(outline_width),
        "outlineOffset" => number!(outline_offset),
        "fontSize" => number!(font_size),
        "lineHeight" => number!(line_height),
        "fontWeight" => {
            if let UiStyleValue::Number(value) = value {
                if value.is_finite() && (0.0..=u16::MAX as f32).contains(&value) {
                    style.font_weight = Some(value as u16);
                    return Ok(());
                }
            }
        }
        "backgroundColor" => color!(background_color),
        "color" => color!(color),
        "borderColor" => color!(border_color),
        "outlineColor" => color!(outline_color),
        "display" => {
            if let UiStyleValue::String(value) = value {
                style.display = Some(match value.as_str() {
                    "block" => Display::Block,
                    "flex" => Display::Flex,
                    "none" => Display::None,
                    _ => return Err(UiMutationError::InvalidStyleValue(name.into())),
                });
                return Ok(());
            }
        }
        "flexDirection" => {
            if let UiStyleValue::String(value) = value {
                style.flex_direction = Some(match value.as_str() {
                    "row" => FlexDirection::Row,
                    "column" => FlexDirection::Column,
                    _ => return Err(UiMutationError::InvalidStyleValue(name.into())),
                });
                return Ok(());
            }
        }
        "fontFamily" => {
            if let UiStyleValue::String(value) = value {
                style.font_family = Some(value);
                return Ok(());
            }
        }
        _ => return Err(UiMutationError::UnsupportedStyle(name.into())),
    }
    Err(UiMutationError::InvalidStyleValue(name.into()))
}

fn clear_style(style: &mut UiStyle, name: &str) -> Result<(), UiMutationError> {
    macro_rules! clear {
        ($field:ident) => {{
            style.$field = None;
            return Ok(());
        }};
    }
    match name {
        "display" => clear!(display),
        "flexDirection" => clear!(flex_direction),
        "width" => clear!(width),
        "height" => clear!(height),
        "minWidth" => clear!(min_width),
        "minHeight" => clear!(min_height),
        "maxWidth" => clear!(max_width),
        "maxHeight" => clear!(max_height),
        "flexGrow" => clear!(flex_grow),
        "flexShrink" => clear!(flex_shrink),
        "gap" => clear!(gap),
        "padding" => clear!(padding),
        "margin" => clear!(margin),
        "backgroundColor" => clear!(background_color),
        "color" => clear!(color),
        "borderColor" => clear!(border_color),
        "borderWidth" => clear!(border_width),
        "borderRadius" => clear!(border_radius),
        "outlineColor" => clear!(outline_color),
        "outlineWidth" => clear!(outline_width),
        "outlineOffset" => clear!(outline_offset),
        "fontSize" => clear!(font_size),
        "lineHeight" => clear!(line_height),
        "fontWeight" => clear!(font_weight),
        "fontFamily" => clear!(font_family),
        _ => Err(UiMutationError::UnsupportedStyle(name.into())),
    }
}
