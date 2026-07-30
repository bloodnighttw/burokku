use super::*;
use crate::ui::{
    elements::{ElementKind, BODY_ID},
    layouts::LayoutKind,
};
use render::{BackgroundImage, DrawCommand};

mod frame;
mod overflow;
mod stacking;

fn ordered_commands(canvas: &Canvas) -> Vec<&DrawCommand> {
    fn append<'a>(canvas: &'a Canvas, commands: &mut Vec<&'a DrawCommand>) {
        for layer in PaintLayer::ALL {
            // This mirrors the renderer's actual per-layer pipeline:
            // shapes are batched first, groups are composited in command
            // order next, and direct text is submitted last.
            for command in canvas.commands() {
                let is_shape = match command {
                    DrawCommand::Decoration {
                        layer: command_layer,
                        ..
                    } => *command_layer == layer,
                    DrawCommand::Box { .. } => layer == PaintLayer::Block,
                    DrawCommand::OverlayBox { .. } => layer == PaintLayer::Overlay,
                    DrawCommand::Text { .. } | DrawCommand::Group { .. } => false,
                };
                if is_shape {
                    commands.push(command);
                }
            }
            for command in canvas.commands() {
                if let DrawCommand::Group {
                    layer: command_layer,
                    canvas,
                    ..
                } = command
                {
                    if *command_layer == layer {
                        append(canvas, commands);
                    }
                }
            }
            if layer == PaintLayer::Content {
                for command in canvas.commands() {
                    if matches!(command, DrawCommand::Text { .. }) {
                        commands.push(command);
                    }
                }
            }
        }
    }

    let mut commands = Vec::new();
    append(canvas, &mut commands);
    commands
}

fn background_colors(canvas: &Canvas) -> Vec<Color> {
    ordered_commands(canvas)
        .into_iter()
        .filter_map(|command| match command {
            DrawCommand::Decoration {
                decoration: BoxDecoration::Background { color, .. },
                ..
            } => Some(*color),
            DrawCommand::Box { style, .. } => Some(style.background),
            _ => None,
        })
        .collect()
}
