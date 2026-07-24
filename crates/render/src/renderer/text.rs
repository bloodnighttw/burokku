use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
};

use glyphon::{Buffer, Cache, Resolution, SwashCache, TextArea, TextAtlas, TextBounds, Viewport};

use crate::{Canvas, Clip, Color, DrawCommand, Rect, TextStyle, TextSystem};

use super::{RenderError, SurfaceSize};

pub(super) struct TextRenderer {
    swash_cache: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    renderer: glyphon::TextRenderer,
    buffers: Vec<CachedText>,
    placements: Vec<TextPlacement>,
}

struct CachedText {
    text: String,
    style: TextStyle,
    width: f32,
    height: f32,
    buffer: Buffer,
}

struct TextPlacement {
    buffer_index: usize,
    bounds: Rect,
    clips: Vec<Clip>,
    color: Color,
}

impl TextRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
    ) -> Self {
        let swash_cache = SwashCache::new();
        let cache = Cache::new(device);
        let viewport = Viewport::new(device, &cache);
        let mut atlas = TextAtlas::new(device, queue, &cache, target_format);
        let renderer =
            glyphon::TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
        Self {
            swash_cache,
            viewport,
            atlas,
            renderer,
            buffers: Vec::new(),
            placements: Vec::new(),
        }
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        canvas: &Canvas,
        size: SurfaceSize,
        text_system: &mut TextSystem,
    ) -> Result<(), RenderError> {
        self.viewport.update(
            queue,
            Resolution {
                width: size.width,
                height: size.height,
            },
        );
        let commands: Vec<_> = canvas
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Text {
                    bounds,
                    text,
                    style,
                    clips,
                } if bounds.width > 0.0 && bounds.height > 0.0 && !text.is_empty() => {
                    Some((*bounds, clips.as_slice(), text.as_str(), style))
                }
                _ => None,
            })
            .collect();
        self.placements.clear();
        self.placements.reserve(commands.len());
        let buffers_match_in_order = self.buffers.len() == commands.len()
            && self
                .buffers
                .iter()
                .zip(&commands)
                .all(|(cached, (bounds, _, text, style))| {
                    cached.matches(text, style, bounds.width, bounds.height)
                });
        if buffers_match_in_order {
            for (buffer_index, (bounds, clips, _, style)) in commands.iter().enumerate() {
                push_text_placements(&mut self.placements, buffer_index, *bounds, clips, style);
            }
        } else {
            let mut previous_buffers: HashMap<u64, Vec<CachedText>> = HashMap::new();
            for cached in std::mem::take(&mut self.buffers) {
                previous_buffers
                    .entry(cached.fingerprint())
                    .or_default()
                    .push(cached);
            }
            self.buffers.reserve(commands.len());
            for (bounds, clips, text, style) in commands {
                let fingerprint = text_layout_fingerprint(text, style, bounds.width, bounds.height);
                let cached = previous_buffers
                    .get_mut(&fingerprint)
                    .and_then(|candidates| {
                        candidates
                            .iter()
                            .position(|cached| {
                                cached.matches(text, style, bounds.width, bounds.height)
                            })
                            .map(|index| candidates.swap_remove(index))
                    })
                    .unwrap_or_else(|| CachedText {
                        text: text.to_owned(),
                        style: style.clone(),
                        width: bounds.width,
                        height: bounds.height,
                        buffer: text_system.layout_buffer(
                            text,
                            style,
                            Some(bounds.width),
                            Some(bounds.height),
                            None,
                        ),
                    });
                let buffer_index = self.buffers.len();
                self.buffers.push(cached);
                push_text_placements(&mut self.placements, buffer_index, bounds, clips, style);
            }
        }

        let areas = self.placements.iter().map(|placement| TextArea {
            buffer: &self.buffers[placement.buffer_index].buffer,
            left: placement.bounds.x,
            top: placement.bounds.y,
            scale: 1.0,
            bounds: clipped_bounds(placement.bounds, &placement.clips, size),
            default_color: glyphon_color(placement.color),
            custom_glyphs: &[],
        });
        self.renderer.prepare(
            device,
            queue,
            text_system.font_system_mut(),
            &mut self.atlas,
            &self.viewport,
            areas,
            &mut self.swash_cache,
        )?;
        Ok(())
    }

    pub fn draw<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>) -> Result<(), RenderError> {
        self.renderer.render(&self.atlas, &self.viewport, pass)?;
        Ok(())
    }

    pub fn finish_frame(&mut self) {
        self.atlas.trim();
    }
}

fn push_text_placements(
    placements: &mut Vec<TextPlacement>,
    buffer_index: usize,
    bounds: Rect,
    clips: &[Clip],
    style: &TextStyle,
) {
    let bounds = transformed_text_bounds(bounds, style);
    if let Some(shadow) = style.shadow {
        let blur = shadow.blur.max(0.0);
        let samples: &[[f32; 2]] = if blur > 0.0 {
            &[
                [0.0, 0.0],
                [-1.0, 0.0],
                [1.0, 0.0],
                [0.0, -1.0],
                [0.0, 1.0],
                [-0.707, -0.707],
                [0.707, -0.707],
                [-0.707, 0.707],
                [0.707, 0.707],
            ]
        } else {
            &[[0.0, 0.0]]
        };
        for sample in samples {
            let mut shadow_bounds = bounds;
            shadow_bounds.x += shadow.offset[0] + sample[0] * blur * 0.5;
            shadow_bounds.y += shadow.offset[1] + sample[1] * blur * 0.5;
            let mut color = color_with_opacity(shadow.color, style.opacity);
            color.alpha /= samples.len() as f32;
            placements.push(TextPlacement {
                buffer_index,
                bounds: shadow_bounds,
                clips: clips.to_vec(),
                color,
            });
        }
    }
    placements.push(TextPlacement {
        buffer_index,
        bounds,
        clips: clips.to_vec(),
        color: color_with_opacity(style.color, style.opacity),
    });
}

fn transformed_text_bounds(bounds: Rect, style: &TextStyle) -> Rect {
    let [a, b, c, d, e, f] = style.transform.matrix;
    let center = [
        bounds.x + bounds.width * 0.5,
        bounds.y + bounds.height * 0.5,
    ];
    let corners = [
        [-bounds.width * 0.5, -bounds.height * 0.5],
        [bounds.width * 0.5, -bounds.height * 0.5],
        [-bounds.width * 0.5, bounds.height * 0.5],
        [bounds.width * 0.5, bounds.height * 0.5],
    ];
    let transformed = corners.map(|point| {
        [
            center[0] + a * point[0] + c * point[1] + e,
            center[1] + b * point[0] + d * point[1] + f,
        ]
    });
    let min_x = transformed
        .iter()
        .map(|point| point[0])
        .fold(f32::INFINITY, f32::min);
    let max_x = transformed
        .iter()
        .map(|point| point[0])
        .fold(f32::NEG_INFINITY, f32::max);
    let min_y = transformed
        .iter()
        .map(|point| point[1])
        .fold(f32::INFINITY, f32::min);
    let max_y = transformed
        .iter()
        .map(|point| point[1])
        .fold(f32::NEG_INFINITY, f32::max);
    Rect::new(min_x, min_y, max_x - min_x, max_y - min_y)
}

fn color_with_opacity(mut color: Color, opacity: f32) -> Color {
    color.alpha *= opacity.clamp(0.0, 1.0);
    color
}

impl CachedText {
    fn matches(&self, text: &str, style: &TextStyle, width: f32, height: f32) -> bool {
        self.text == text
            && self.width == width
            && self.height == height
            && text_layout_style_matches(&self.style, style)
    }

    fn fingerprint(&self) -> u64 {
        text_layout_fingerprint(&self.text, &self.style, self.width, self.height)
    }
}

fn text_layout_style_matches(left: &TextStyle, right: &TextStyle) -> bool {
    left.font_size == right.font_size
        && left.line_height == right.line_height
        && left.font_weight == right.font_weight
        && left.font_families == right.font_families
        && left.font_style == right.font_style
        && left.text_align == right.text_align
        && left.letter_spacing == right.letter_spacing
        && left.word_spacing == right.word_spacing
        && left.wrap == right.wrap
}

fn text_layout_fingerprint(text: &str, style: &TextStyle, width: f32, height: f32) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    style.font_size.to_bits().hash(&mut hasher);
    style.line_height.to_bits().hash(&mut hasher);
    style.font_weight.hash(&mut hasher);
    style.font_families.hash(&mut hasher);
    style.font_style.hash(&mut hasher);
    style.text_align.hash(&mut hasher);
    style.letter_spacing.to_bits().hash(&mut hasher);
    style.word_spacing.to_bits().hash(&mut hasher);
    style.wrap.hash(&mut hasher);
    width.to_bits().hash(&mut hasher);
    height.to_bits().hash(&mut hasher);
    hasher.finish()
}

fn clipped_bounds(mut bounds: Rect, clips: &[Clip], size: SurfaceSize) -> TextBounds {
    for clip in clips {
        bounds = bounds.intersection(clip.rect);
    }
    TextBounds {
        left: bounds.x.floor().max(0.0) as i32,
        top: bounds.y.floor().max(0.0) as i32,
        right: (bounds.x + bounds.width).ceil().min(size.width as f32) as i32,
        bottom: (bounds.y + bounds.height).ceil().min(size.height as f32) as i32,
    }
}

fn glyphon_color(color: Color) -> glyphon::Color {
    let [red, green, blue, alpha] = color.rgba8();
    glyphon::Color::rgba(red, green, blue, alpha)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_layout_cache_ignores_paint_only_color_changes() {
        let left = TextStyle {
            color: Color::BLACK,
            ..TextStyle::default()
        };
        let right = TextStyle {
            color: Color::WHITE,
            ..left.clone()
        };

        assert!(text_layout_style_matches(&left, &right));
    }

    #[test]
    fn text_layout_cache_detects_shape_changes() {
        let left = TextStyle::default();
        let right = TextStyle {
            font_size: left.font_size + 1.0,
            ..left.clone()
        };

        assert!(!text_layout_style_matches(&left, &right));
    }
}
