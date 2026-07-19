use glyphon::{
    Attrs, Buffer, Cache, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache, TextArea,
    TextAtlas, TextBounds, Viewport, Weight, Wrap,
};

use crate::{Canvas, Color, DrawCommand, FontFamily, Rect, TextWrap};

use super::{RenderError, SurfaceSize};

pub(super) struct TextRenderer {
    font_system: FontSystem,
    swash_cache: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    renderer: glyphon::TextRenderer,
}

impl TextRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
    ) -> Self {
        let font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let cache = Cache::new(device);
        let viewport = Viewport::new(device, &cache);
        let mut atlas = TextAtlas::new(device, queue, &cache, target_format);
        let renderer =
            glyphon::TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
        Self {
            font_system,
            swash_cache,
            viewport,
            atlas,
            renderer,
        }
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        canvas: &Canvas,
        size: SurfaceSize,
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
                } if bounds.width > 0.0 && bounds.height > 0.0 && !text.is_empty() => {
                    Some((*bounds, text.as_str(), style))
                }
                _ => None,
            })
            .collect();
        let mut buffers = Vec::with_capacity(commands.len());
        for (bounds, text, style) in &commands {
            let font_size = style.font_size.max(1.0);
            let line_height = style.line_height.max(font_size);
            let mut buffer =
                Buffer::new(&mut self.font_system, Metrics::new(font_size, line_height));
            buffer.set_size(
                &mut self.font_system,
                Some(bounds.width),
                Some(bounds.height),
            );
            buffer.set_wrap(
                &mut self.font_system,
                match style.wrap {
                    TextWrap::None => Wrap::None,
                    TextWrap::Glyph => Wrap::Glyph,
                    TextWrap::Word => Wrap::Word,
                },
            );
            let family = match &style.font_family {
                FontFamily::SansSerif => Family::SansSerif,
                FontFamily::Serif => Family::Serif,
                FontFamily::Monospace => Family::Monospace,
                FontFamily::Named(name) => Family::Name(name),
            };
            let attrs = Attrs::new()
                .family(family)
                .weight(Weight(style.font_weight));
            buffer.set_text(&mut self.font_system, text, &attrs, Shaping::Advanced, None);
            buffer.shape_until_scroll(&mut self.font_system, false);
            buffers.push(buffer);
        }
        let areas = buffers
            .iter()
            .zip(&commands)
            .map(|(buffer, (bounds, _, style))| TextArea {
                buffer,
                left: bounds.x,
                top: bounds.y,
                scale: 1.0,
                bounds: clipped_bounds(*bounds, size),
                default_color: glyphon_color(style.color),
                custom_glyphs: &[],
            });
        self.renderer.prepare(
            device,
            queue,
            &mut self.font_system,
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

fn clipped_bounds(bounds: Rect, size: SurfaceSize) -> TextBounds {
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
