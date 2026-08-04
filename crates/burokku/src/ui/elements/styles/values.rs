mod background_image;
mod font_style;
mod gradient_stop;
mod isolation;
mod length;
mod length_percentage;
mod line_height;
mod max_size;
mod overflow;
mod overflow_wrap;
mod position;
mod shadow;
mod size;
mod text_align;
mod text_decoration_line;
mod transform;
mod white_space;
mod word_break;
mod z_index;

pub(crate) use background_image::BackgroundImage;
pub(crate) use font_style::FontStyleValue;
pub(crate) use gradient_stop::GradientStop;
pub(crate) use isolation::Isolation;
pub(crate) use length::LengthValue;
pub(crate) use length_percentage::LengthPercentageValue;
pub(crate) use line_height::LineHeightValue;
pub(crate) use max_size::MaxSizeValue;
pub(crate) use overflow::Overflow;
pub(crate) use overflow_wrap::OverflowWrapValue;
pub use position::Position;
pub(crate) use shadow::Shadow;
pub(crate) use size::SizeValue;
pub(crate) use text_align::TextAlignValue;
pub(crate) use text_decoration_line::TextDecorationLineValue;
pub(crate) use transform::Transform;
pub(crate) use white_space::WhiteSpaceValue;
pub(crate) use word_break::WordBreakValue;
pub(crate) use z_index::ZIndex;

fn rgba(color: [u8; 4]) -> render::Color {
    render::Color::from_rgba8(color[0], color[1], color[2], color[3])
}
