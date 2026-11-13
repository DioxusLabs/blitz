//! The renderers compiled into the runner, chosen at runtime with `--renderer`.

#[cfg(not(any(feature = "cpu", feature = "tiny-skia", feature = "gpu")))]
compile_error!("enable at least one of the `cpu`, `tiny-skia` and `gpu` features");

use anyrender::ImageRenderer;

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
}

/// `ImageRenderer::render_to_vec` on whichever renderer a [`Renderer`] holds. A macro rather than
/// a method so that the drawing closure is instantiated for each renderer's scene painter.
macro_rules! render_to_vec {
    ($renderer:expr, $draw:expr, $buffer:expr $(,)?) => {
        match &mut $renderer {
            #[cfg(feature = "cpu")]
            $crate::renderer::Renderer::VelloCpu(renderer) => {
                anyrender::ImageRenderer::render_to_vec(renderer, $draw, $buffer)
            }
            #[cfg(feature = "tiny-skia")]
            $crate::renderer::Renderer::TinySkia(renderer) => {
                anyrender::ImageRenderer::render_to_vec(renderer, $draw, $buffer)
            }
            #[cfg(feature = "gpu")]
            $crate::renderer::Renderer::Vello(renderer) => {
                anyrender::ImageRenderer::render_to_vec(renderer, $draw, $buffer)
            }
        }
    };
}
pub(crate) use render_to_vec;
