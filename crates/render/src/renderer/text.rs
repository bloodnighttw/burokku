use glyphon::{Buffer, Cache, Resolution, SwashCache, TextArea, TextAtlas, TextBounds, Viewport};

use crate::{Canvas, Clip, Color, DrawCommand, Rect, TextStyle, TextSystem};

use super::{RenderError, SurfaceSize};

pub(super) struct TextRenderer {
    swash_cache: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    renderer: glyphon::TextRenderer,
    buffers: Vec<CachedText>,
}

struct CachedText {
    bounds: Rect,
    clips: Vec<Clip>,
    text: String,
    style: TextStyle,
    buffer: Buffer,
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
        let buffers_match =
            self.buffers.len() == commands.len()
                && self.buffers.iter().zip(&commands).all(
                    |(cached, (bounds, clips, text, style))| {
                        cached.bounds == *bounds
                            && cached.clips == *clips
                            && cached.text == *text
                            && cached.style == **style
                    },
                );
        if !buffers_match {
            self.buffers.clear();
            self.buffers.reserve(commands.len());
            for (bounds, clips, text, style) in &commands {
                self.buffers.push(CachedText {
                    bounds: *bounds,
                    clips: clips.to_vec(),
                    text: (*text).to_owned(),
                    style: (*style).clone(),
                    buffer: text_system.layout_buffer(
                        text,
                        style,
                        Some(bounds.width),
                        Some(bounds.height),
                        None,
                    ),
                });
            }
        }

        let areas = self.buffers.iter().map(|cached| TextArea {
            buffer: &cached.buffer,
            left: cached.bounds.x,
            top: cached.bounds.y,
            scale: 1.0,
            bounds: clipped_bounds(cached.bounds, &cached.clips, size),
            default_color: glyphon_color(cached.style.color),
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
