//! The renderers compiled into the runner, chosen at runtime with `--renderer`.

#[cfg(not(any(feature = "cpu", feature = "tiny-skia", feature = "gpu")))]
compile_error!("enable at least one of the `cpu`, `tiny-skia` and `gpu` features");

use anyrender::{ImageRenderer, PaintScene};
use blitz_dom::BaseDocument;
use blitz_dom::util::Color;
use blitz_paint::paint_scene;
use peniko::Fill;
use peniko::kurbo::Rect;

use crate::{HEIGHT, SCALE, WIDTH};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RendererKind {
    #[cfg(feature = "cpu")]
    VelloCpu,
    #[cfg(feature = "tiny-skia")]
    TinySkia,
    #[cfg(feature = "gpu")]
    Vello,
}

impl RendererKind {
    /// The compiled-in renderers. The first is the default.
    pub fn all() -> Vec<Self> {
        vec![
            #[cfg(feature = "cpu")]
            Self::VelloCpu,
            #[cfg(feature = "tiny-skia")]
            Self::TinySkia,
            #[cfg(feature = "gpu")]
            Self::Vello,
        ]
    }

    pub fn name(self) -> &'static str {
        match self {
            #[cfg(feature = "cpu")]
            Self::VelloCpu => "vello-cpu",
            #[cfg(feature = "tiny-skia")]
            Self::TinySkia => "tiny-skia",
            #[cfg(feature = "gpu")]
            Self::Vello => "vello",
        }
    }
}

/// Splits `--renderer <names>` (or `--renderer=<names>`) out of the command line arguments.
///
/// `<names>` is a comma-separated list of renderers, or `all`. With more than one, each test is
/// rendered by every one of them in turn. Returns the selected renderers (the default one if
/// there is no `--renderer`) and the remaining non-flag arguments.
pub fn parse_args(args: impl IntoIterator<Item = String>) -> (Vec<RendererKind>, Vec<String>) {
    let mut names = None;
    let mut rest = Vec::new();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--renderer" {
            names = args.next();
        } else if let Some(value) = arg.strip_prefix("--renderer=") {
            names = Some(value.to_string());
        } else if !arg.starts_with('-') {
            rest.push(arg);
        }
    }

    let all = RendererKind::all();
    let kinds = match names.as_deref() {
        None => vec![all[0]],
        Some("all") => all,
        Some(names) => names
            .split(',')
            .map(|name| {
                all.iter()
                    .copied()
                    .find(|kind| kind.name() == name)
                    .unwrap_or_else(|| {
                        let available: Vec<_> = all.iter().map(|kind| kind.name()).collect();
                        eprintln!(
                            "Unknown renderer {name:?}. Available: {}, all",
                            available.join(", ")
                        );
                        std::process::exit(1);
                    })
            })
            .collect(),
    };
    (kinds, rest)
}

// One per worker thread, so the size difference between variants doesn't matter.
#[allow(clippy::large_enum_variant)]
pub enum Renderer {
    #[cfg(feature = "cpu")]
    VelloCpu(anyrender_vello_cpu::VelloCpuImageRenderer),
    #[cfg(feature = "tiny-skia")]
    TinySkia(anyrender_tiny_skia::TinySkiaImageRenderer),
    #[cfg(feature = "gpu")]
    Vello(anyrender_vello::VelloImageRenderer),
}

impl Renderer {
    pub fn new(kind: RendererKind, width: u32, height: u32) -> Self {
        match kind {
            #[cfg(feature = "cpu")]
            RendererKind::VelloCpu => Self::VelloCpu(ImageRenderer::new(width, height)),
            #[cfg(feature = "tiny-skia")]
            RendererKind::TinySkia => Self::TinySkia(ImageRenderer::new(width, height)),
            #[cfg(feature = "gpu")]
            RendererKind::Vello => Self::Vello(ImageRenderer::new(width, height)),
        }
    }

    /// Renders `document` on a white background into `buffer`.
    pub fn render_to_vec(&mut self, document: &mut BaseDocument, buffer: &mut Vec<u8>) {
        match self {
            #[cfg(feature = "cpu")]
            Self::VelloCpu(renderer) => {
                renderer.render_to_vec(|scene| draw(scene, document), buffer)
            }
            #[cfg(feature = "tiny-skia")]
            Self::TinySkia(renderer) => {
                renderer.render_to_vec(|scene| draw(scene, document), buffer)
            }
            #[cfg(feature = "gpu")]
            Self::Vello(renderer) => renderer.render_to_vec(|scene| draw(scene, document), buffer),
        }
    }
}

fn draw(scene: &mut impl PaintScene, document: &mut BaseDocument) {
    scene.reset();
    scene.fill(
        Fill::NonZero,
        Default::default(),
        Color::WHITE,
        Default::default(),
        &Rect::new(0.0, 0.0, WIDTH as f64, HEIGHT as f64),
    );
    paint_scene(scene, document, SCALE, WIDTH, HEIGHT, 0, 0);
}
