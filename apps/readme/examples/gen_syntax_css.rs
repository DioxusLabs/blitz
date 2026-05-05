use std::fs;
use std::path::Path;
use syntect::highlighting::ThemeSet;
use syntect::html::{ClassStyle, css_for_theme_with_class_style};

fn main() {
    let themes = ThemeSet::load_defaults();
    let light = css_for_theme_with_class_style(
        &themes.themes["InspiredGitHub"], ClassStyle::Spaced).unwrap();
    let dark = css_for_theme_with_class_style(
        &themes.themes["base16-ocean.dark"], ClassStyle::Spaced).unwrap();
    let css = format!(
        "/* Generated from syntect themes. Re-run apps/readme/examples/gen_syntax_css.rs to refresh. */\n\
         @media (prefers-color-scheme: light) {{\n{light}\n}}\n\
         @media (prefers-color-scheme: dark) {{\n{dark}\n}}\n"
    );
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/syntax-highlight.css");
    fs::write(&out, css).unwrap();
    eprintln!("wrote {}", out.display());
}
