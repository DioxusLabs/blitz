//! Renders the editor to a PNG without opening a window.
//!
//! Usage: `cargo run -p parley_code_editor --example screenshot [--wrap] [file] [output.png]`

use std::fs::File;
use std::io::BufWriter;

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use kurbo::{Affine, Point, Size};
use parley_code_editor::{EditorInput, Key, KeyInput, Modifiers, PointerButton, TextContext};

mod common;

const WIDTH: f64 = 900.0;
const HEIGHT: f64 = 600.0;
const SCALE: f64 = 2.0;

fn main() {
    let mut wrap = false;
    let mut args = Vec::new();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--wrap" => wrap = true,
            _ => args.push(arg),
        }
    }
    let mut args = args.into_iter();
    let path = args.next();
    let out_path = args.next().unwrap_or_else(|| "editor.png".into());

    let mut cx = TextContext::new();
    let mut editor = common::open(path.as_deref(), wrap);
    editor.set_size(Size::new(WIDTH, HEIGHT));
    editor.handle_input(&mut cx, EditorInput::Focus(true));

    // Select a word on the fourth line, and add a second caret below it
    let line_height = editor.style().font_size as f64 * editor.style().line_height as f64;
    editor.handle_input(
        &mut cx,
        EditorInput::PointerDown {
            pos: Point::new(120.0, line_height * 3.5),
            button: PointerButton::Primary,
            mods: Modifiers::default(),
            click_count: 2,
        },
    );
    editor.handle_input(
        &mut cx,
        EditorInput::PointerDown {
            pos: Point::new(120.0, line_height * 5.5),
            button: PointerButton::Primary,
            mods: Modifiers {
                alt: true,
                ..Default::default()
            },
            click_count: 1,
        },
    );
    editor.handle_input(
        &mut cx,
        EditorInput::Key(KeyInput {
            key: Key::ArrowDown,
            mods: Modifiers {
                shift: true,
                ..Default::default()
            },
            text: None,
        }),
    );

    let width = (WIDTH * SCALE) as u32;
    let height = (HEIGHT * SCALE) as u32;
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| editor.paint(&mut cx, scene, Affine::scale(SCALE)),
        width,
        height,
    );

    let file = BufWriter::new(File::create(&out_path).unwrap());
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&buffer).unwrap();
    writer.finish().unwrap();
    println!("Written to {out_path}");
}
