//! This module contain the ** uncomputed ** node of ui, which is called as
//! **Elements**

use crate::ui::elements::styles::Style;

mod styles;

const BODY_ID: u64 = 0;

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

pub struct Element {
    pub kind: ElementKind,
    pub parent: Option<u64>,
    pub children: Vec<u64>,
    pub style: Style
}