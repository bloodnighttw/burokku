#[derive(Debug, Clone, PartialEq, PartialOrd, Default)]
pub struct Stroke {
    pub x: f32,
    pub y: f32,
    pub path: Vec<(f32, f32)>,
    pub width: f32,
}
