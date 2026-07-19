/// An sRGB color with an unassociated (straight) alpha channel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub red: f32,
    pub green: f32,
    pub blue: f32,
    pub alpha: f32,
}

impl Color {
    pub const TRANSPARENT: Self = Self::rgba(0.0, 0.0, 0.0, 0.0);
    pub const BLACK: Self = Self::rgb(0.0, 0.0, 0.0);
    pub const WHITE: Self = Self::rgb(1.0, 1.0, 1.0);

    pub const fn rgb(red: f32, green: f32, blue: f32) -> Self {
        Self::rgba(red, green, blue, 1.0)
    }

    pub const fn rgba(red: f32, green: f32, blue: f32, alpha: f32) -> Self {
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }

    pub const fn from_rgba8(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self::rgba(
            red as f32 / 255.0,
            green as f32 / 255.0,
            blue as f32 / 255.0,
            alpha as f32 / 255.0,
        )
    }

    pub(crate) fn components(self) -> [f32; 4] {
        [
            self.red.clamp(0.0, 1.0),
            self.green.clamp(0.0, 1.0),
            self.blue.clamp(0.0, 1.0),
            self.alpha.clamp(0.0, 1.0),
        ]
    }

    pub(crate) fn rgba8(self) -> [u8; 4] {
        self.components().map(|value| (value * 255.0).round() as u8)
    }

    pub(crate) fn as_wgpu_clear(self) -> wgpu::Color {
        fn linear(channel: f32) -> f64 {
            let channel = channel.clamp(0.0, 1.0) as f64;
            if channel <= 0.04045 {
                channel / 12.92
            } else {
                ((channel + 0.055) / 1.055).powf(2.4)
            }
        }

        wgpu::Color {
            r: linear(self.red),
            g: linear(self.green),
            b: linear(self.blue),
            a: self.alpha.clamp(0.0, 1.0) as f64,
        }
    }
}
