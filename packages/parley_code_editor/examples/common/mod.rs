//! Setup shared by the examples

use std::path::Path;

use parley_code_editor::{CodeEditor, EditorStyle, SyntaxmateHighlighter};

/// Opens `path` (or the editor example's own source) in an editor, with syntax highlighting
/// and colors from the GitHub Dark theme
pub fn open(path: Option<&str>, soft_wrap: bool) -> CodeEditor {
    let (text, language) = match path {
        Some(path) => {
            let text = std::fs::read_to_string(path).expect("Failed to read file");
            let language = SyntaxmateHighlighter::detect_language(Some(Path::new(path)), &text);
            (text, language)
        }
        None => (
            include_str!("../editor.rs").to_string(),
            Some("rust".into()),
        ),
    };
    let mut editor = CodeEditor::new(&text);
    editor.set_style(EditorStyle {
        soft_wrap,
        ..Default::default()
    });
    match language.map(|language| SyntaxmateHighlighter::new(&language, "github-dark")) {
        Some(Ok(highlighter)) => {
            editor.set_theme(highlighter.editor_theme());
            editor.set_highlighter(highlighter);
        }
        Some(Err(err)) => eprintln!("No syntax highlighting: {err}"),
        None => {}
    }
    editor
}
