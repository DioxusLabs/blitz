use peniko::Color;

/// Font and spacing settings for a [`CodeEditor`](crate::CodeEditor)
#[derive(Clone, Debug, PartialEq)]
pub struct EditorStyle {
    /// Font family list, in CSS `font-family` syntax
    pub font_family: String,
    /// Font size in logical pixels
    pub font_size: f32,
    /// Line height as a multiple of the font size
    pub line_height: f32,
    /// Number of columns between tab stops
    pub tab_size: usize,
    /// Whether to show the line number gutter
    pub show_gutter: bool,
    /// Whether to wrap long lines at the edge of the editor instead of scrolling horizontally
    pub soft_wrap: bool,
}

impl Default for EditorStyle {
    fn default() -> Self {
        Self {
            font_family: "ui-monospace, Menlo, Consolas, \"DejaVu Sans Mono\", monospace".into(),
            font_size: 14.0,
            line_height: 1.5,
            tab_size: 4,
            show_gutter: true,
            soft_wrap: false,
        }
    }
}

/// Colors for a [`CodeEditor`](crate::CodeEditor)
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub background: Color,
    pub foreground: Color,
    pub gutter_background: Color,
    pub gutter_foreground: Color,
    pub gutter_active_foreground: Color,
    pub current_line: Color,
    pub selection: Color,
    pub caret: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            background: Color::from_rgb8(0x28, 0x2c, 0x34),
            foreground: Color::from_rgb8(0xab, 0xb2, 0xbf),
            gutter_background: Color::from_rgb8(0x28, 0x2c, 0x34),
            gutter_foreground: Color::from_rgb8(0x4b, 0x52, 0x63),
            gutter_active_foreground: Color::from_rgb8(0xab, 0xb2, 0xbf),
            current_line: Color::from_rgba8(0xff, 0xff, 0xff, 0x0a),
            selection: Color::from_rgba8(0x3e, 0x44, 0x51, 0xff),
            caret: Color::from_rgb8(0x52, 0x8b, 0xff),
        }
    }
}
