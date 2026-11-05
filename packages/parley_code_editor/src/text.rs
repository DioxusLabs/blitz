use parley::{FontContext, LayoutContext};
use peniko::Color;

/// The Parley brush type used for editor text
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextBrush(pub Color);

impl Default for TextBrush {
    fn default() -> Self {
        Self(Color::BLACK)
    }
}

/// The Parley contexts used to lay out editor text. A single `TextContext` can be shared by
/// several editors.
pub struct TextContext {
    pub font_cx: FontContext,
    pub layout_cx: LayoutContext<TextBrush>,
}

impl TextContext {
    pub fn new() -> Self {
        Self {
            font_cx: FontContext::new(),
            layout_cx: LayoutContext::new(),
        }
    }
}

impl Default for TextContext {
    fn default() -> Self {
        Self::new()
    }
}
