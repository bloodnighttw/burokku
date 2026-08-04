use render::BackgroundImage as RenderBackgroundImage;

use super::GradientStop;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BackgroundImage {
    LinearGradient {
        direction: [f32; 2],
        stops: Vec<GradientStop>,
    },
    RadialGradient {
        stops: Vec<GradientStop>,
    },
    Raster(render::RasterImage),
}

impl From<BackgroundImage> for RenderBackgroundImage {
    fn from(value: BackgroundImage) -> Self {
        match value {
            BackgroundImage::LinearGradient { direction, stops } => Self::LinearGradient {
                direction,
                stops: stops.into_iter().map(Into::into).collect(),
            },
            BackgroundImage::RadialGradient { stops } => Self::RadialGradient {
                stops: stops.into_iter().map(Into::into).collect(),
            },
            BackgroundImage::Raster(image) => Self::Raster(image),
        }
    }
}
