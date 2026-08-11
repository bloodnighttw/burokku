use std::{env, fs::File, io, path::PathBuf};

use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
use render::{
    canvas::DrawList,
    offscreen::OffscreenSurface,
    shapes::{
        rect::{DrawRectExt, Rect},
        round::Round,
        stroke::{DrawStrokeExt, Stroke},
    },
    wgpu,
};

const SIZE: [u32; 2] = [800, 500];

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("render-shapes.png"));
    let mut surface = OffscreenSurface::new(SIZE).await.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "no compatible WebGPU adapter found",
        )
    })?;
    let draws = scene();
    let rgba = surface.render_rgba8(&draws, color(15, 23, 42)).await;

    let file = File::create(&output)?;
    PngEncoder::new(file).write_image(&rgba, SIZE[0], SIZE[1], ExtendedColorType::Rgba8)?;

    println!("wrote {}", output.display());
    Ok(())
}

fn scene() -> DrawList {
    let mut draws = DrawList::new();

    draws.draw_rounded_rect(
        Rect::new(70.0, 55.0, 660.0, 390.0),
        color(248, 250, 252),
        rounded(32.0),
    );
    draws.draw_rounded_rect(
        Rect::new(105.0, 90.0, 590.0, 92.0),
        color(30, 41, 59),
        rounded(22.0),
    );

    draws.with_rounded_clip(
        Rect::new(105.0, 212.0, 380.0, 198.0),
        rounded(24.0),
        |draws| {
            draws.draw_rect(Rect::new(105.0, 212.0, 380.0, 198.0), color(224, 231, 255));
            draws.draw_rounded_rect(
                Rect::new(68.0, 244.0, 220.0, 220.0),
                color(99, 102, 241),
                rounded(110.0),
            );
            draws.draw_rounded_rect(
                Rect::new(305.0, 177.0, 240.0, 240.0),
                color(45, 212, 191),
                rounded(120.0),
            );
        },
    );

    draws.draw_rounded_rect(
        Rect::new(520.0, 212.0, 175.0, 88.0),
        color(254, 226, 226),
        rounded(20.0),
    );
    draws.draw_rounded_rect(
        Rect::new(520.0, 322.0, 175.0, 88.0),
        color(254, 243, 199),
        rounded(20.0),
    );
    draws.draw_rounded_stroke(
        Stroke::new(70.0, 55.0, 660.0, 390.0, 3.0),
        color(148, 163, 184),
        rounded(32.0),
    );

    draws
}

const fn rounded(radius: f32) -> Round {
    Round {
        lt: radius,
        rt: radius,
        rb: radius,
        lb: radius,
    }
}

const fn color(red: u8, green: u8, blue: u8) -> wgpu::Color {
    wgpu::Color {
        r: red as f64 / 255.0,
        g: green as f64 / 255.0,
        b: blue as f64 / 255.0,
        a: 1.0,
    }
}
