//! [`SyntaxHighlighter`] backed by [syntaxmate](https://github.com/phongndo/syntaxmate), a pure
//! Rust TextMate grammar engine. syntaxmate types stay inside this module.

use std::collections::HashMap;
use std::path::Path;

use peniko::Color;
use syntaxmate::{
    Catalog, FontModifiers, PreparedLanguage, RgbColor, ScopeStackId, Style, Theme, Token,
    Tokenizer, TokenizerOptions, TokenizerState,
};

use super::{SpanStyle, StyledSpan, SyntaxHighlighter};

/// Highlights one language with a VS Code (TextMate) theme
pub struct SyntaxmateHighlighter {
    tokenizer: Tokenizer,
    theme: Theme,
    tokens: Vec<Token>,
    /// Resolved styles by scope stack, since many tokens share one
    styles: HashMap<ScopeStackId, SpanStyle>,
}

impl SyntaxmateHighlighter {
    /// Creates a highlighter for a bundled language (e.g. `"rust"`) and bundled theme
    /// (e.g. `"github-dark"`)
    pub fn new(
        language: &str,
        theme: &str,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let tokenizer = PreparedLanguage::for_bundled_language(language)?
            .tokenizer(TokenizerOptions::default());
        Ok(Self {
            tokenizer,
            theme: Theme::bundled(theme)?,
            tokens: Vec::new(),
            styles: HashMap::new(),
        })
    }

    /// Detects the language from a file path, falling back to the text (e.g. a shebang line)
    pub fn detect_language(path: Option<&Path>, text: &str) -> Option<String> {
        Catalog::bundled().detect(path, text).map(str::to_owned)
    }

    /// Editor colors from the theme, for themes that define them
    pub fn editor_theme(&self) -> crate::Theme {
        let color = |name: &str| self.theme.color(name).map(to_color);
        let default = crate::Theme::default();
        let background = color("editor.background").unwrap_or(default.background);
        let foreground = color("editor.foreground").unwrap_or(default.foreground);
        crate::Theme {
            background,
            foreground,
            gutter_background: color("editorGutter.background").unwrap_or(background),
            gutter_foreground: color("editorLineNumber.foreground")
                .unwrap_or(default.gutter_foreground),
            gutter_active_foreground: color("editorLineNumber.activeForeground")
                .unwrap_or(foreground),
            // syntaxmate reports opaque colors, so add back the transparency these usually have
            current_line: color("editor.lineHighlightBackground")
                .map_or(default.current_line, |c| c.with_alpha(0.1)),
            selection: color("editor.selectionBackground")
                .map_or(default.selection, |c| c.with_alpha(0.3)),
            caret: color("editorCursor.foreground").unwrap_or(default.caret),
        }
    }

    fn style(&mut self, token: &Token) -> SpanStyle {
        let resolve = |theme: &Theme| to_span_style(theme.resolve(token.scopes()));
        match token.scope_stack() {
            Some(id) => *self
                .styles
                .entry(id)
                .or_insert_with(|| resolve(&self.theme)),
            None => resolve(&self.theme),
        }
    }
}

impl SyntaxHighlighter for SyntaxmateHighlighter {
    type State = TokenizerState;

    fn initial_state(&self) -> TokenizerState {
        self.tokenizer.initial_state()
    }

    fn highlight_line(
        &mut self,
        line: &str,
        state: &mut TokenizerState,
        spans: &mut Vec<StyledSpan>,
    ) {
        let mut tokens = std::mem::take(&mut self.tokens);
        // A degraded status means a resource limit cut tokenizing short. The spans it did produce
        // are still worth showing. An error leaves the line unstyled.
        if self
            .tokenizer
            .tokenize_line_into(line, state, &mut tokens)
            .is_ok()
        {
            let default = to_span_style(self.theme.default_style());
            for token in &tokens {
                let style = self.style(token);
                if style != default {
                    spans.push(StyledSpan {
                        range: token.range(),
                        style,
                    });
                }
            }
        }
        self.tokens = tokens;
    }
}

fn to_color(color: RgbColor) -> Color {
    Color::from_rgb8(color.red, color.green, color.blue)
}

fn to_span_style(style: Style) -> SpanStyle {
    SpanStyle {
        foreground: style.foreground.map(to_color),
        bold: style.modifiers.contains(FontModifiers::BOLD),
        italic: style.modifiers.contains(FontModifiers::ITALIC),
        underline: style.modifiers.contains(FontModifiers::UNDERLINED),
    }
}
