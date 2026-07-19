use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiDocument {
    pub root: UiNode,
}

impl UiDocument {
    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        serde_json::from_str(json)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiNode {
    pub id: u64,
    #[serde(rename = "type")]
    pub kind: ElementKind,
    #[serde(default)]
    pub style: UiStyle,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub children: Vec<UiNode>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ElementKind {
    Div,
    Button,
    Span,
    Text,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
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

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Display {
    Block,
    Flex,
    None,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FlexDirection {
    Row,
    Column,
}
