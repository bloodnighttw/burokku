#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Corner {
    pub lt: f32,
    pub rt: f32,
    pub br: f32,
    pub bl: f32,
}

impl Corner {
    pub fn new(lt: f32, rt: f32, br: f32, bl: f32) -> Self {
        Self { lt, rt, br, bl }
    }

    pub fn is_zero(&self) -> bool {
        self.lt == 0.0 && self.rt == 0.0 && self.br == 0.0 && self.bl == 0.0
    }

    pub fn left(&mut self, left: f32) {
        self.lt = left;
        self.bl = left;
    }

    pub fn right(&mut self, right: f32) {
        self.rt = right;
        self.br = right;
    }

    pub fn top(&mut self, top: f32) {
        self.lt = top;
        self.rt = top;
    }

    pub fn bottom(&mut self, bottom: f32) {
        self.bl = bottom;
        self.br = bottom;
    }
}
