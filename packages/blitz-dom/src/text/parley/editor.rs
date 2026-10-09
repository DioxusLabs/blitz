//! The text of an `<input>` or `<textarea>`, edited and laid out by Parley's `PlainEditor`.

use std::ops::Range;

use blitz_traits::node_id::NodeId;
use kurbo::{Rect, Size};
use parley::{PlainEditor, StyleProperty};
use style::properties::ComputedValues;

use super::{TextBrush, TextContext, style as stylo_to_parley};
use crate::text::{Edit, EditEngine, EditableText, Motion};

/// The text of an `<input>` or `<textarea>`, edited and laid out by Parley.
pub struct TextEditor {
    editor: PlainEditor<TextBrush>,
}

impl TextEditor {
    /// Parley's editor, which the renderer paints the text, the selection and the caret of.
    pub fn plain_editor(&self) -> &PlainEditor<TextBrush> {
        &self.editor
    }
}

impl EditableText for TextEditor {
    fn raw_text(&self) -> &str {
        self.editor.raw_text()
    }

    fn text(&self) -> String {
        self.editor.text().to_string()
    }

    fn selection(&self) -> Range<usize> {
        self.editor.raw_selection().text_range()
    }

    fn selected_text(&self) -> Option<&str> {
        self.editor.selected_text()
    }

    fn is_selection_collapsed(&self) -> bool {
        self.editor.raw_selection().is_collapsed()
    }

    fn scale(&self) -> f32 {
        self.editor
            .try_layout()
            .map_or(1.0, |layout| layout.scale())
    }

    fn size(&self) -> Option<Size> {
        let layout = self.editor.try_layout()?;
        Some(Size::new(
            f64::from(layout.full_width()),
            f64::from(layout.height()),
        ))
    }

    fn caret_rect(&self) -> Option<Rect> {
        let caret = self.editor.cursor_geometry(1.5)?;
        Some(Rect::new(caret.x0, caret.y0, caret.x1, caret.y1))
    }
}

impl EditEngine for TextEditor {
    fn new(_is_multiline: bool) -> Self {
        Self {
            editor: PlainEditor::new(16.0),
        }
    }

    fn set_initial_text(&mut self, text: &str) {
        self.editor.set_text(text);
    }

    fn set_text(&mut self, cx: &mut TextContext, text: &str) {
        if self.editor.text() != text {
            self.editor.set_text(text);
            self.editor
                .driver(&mut cx.font_ctx.lock().unwrap(), &mut cx.layout_ctx)
                .refresh_layout();
        }
    }

    fn set_style(
        &mut self,
        cx: &mut TextContext,
        node: NodeId,
        style: Option<&ComputedValues>,
        scale: f32,
    ) {
        let parley_style = style
            .map(|s| stylo_to_parley::style(node, s))
            .unwrap_or_default();
        let alignment = style
            .map(|s| stylo_to_parley::text_align(s.clone_text_align()))
            .unwrap_or(parley::layout::Alignment::Start);
        let base_direction = style
            .map(|s| stylo_to_parley::base_direction(s.clone_direction(), s.clone_unicode_bidi()))
            .unwrap_or(parley::BaseDirection::Auto);

        let editor = &mut self.editor;
        editor.set_scale(scale);
        editor.set_width(None);

        let styles = editor.edit_styles();
        styles.retain(|_| false);
        styles.insert(StyleProperty::FontFamily(parley_style.font_family));
        styles.insert(StyleProperty::FontSize(parley_style.font_size));
        styles.insert(StyleProperty::FontWidth(parley_style.font_width));
        styles.insert(StyleProperty::FontStyle(parley_style.font_style));
        styles.insert(StyleProperty::FontWeight(parley_style.font_weight));
        styles.insert(StyleProperty::FontVariations(parley_style.font_variations));
        styles.insert(StyleProperty::FontFeatures(parley_style.font_features));
        styles.insert(StyleProperty::LineHeight(parley_style.line_height));
        styles.insert(StyleProperty::WordSpacing(parley_style.word_spacing));
        styles.insert(StyleProperty::LetterSpacing(parley_style.letter_spacing));
        styles.insert(StyleProperty::WordBreak(parley_style.word_break));
        styles.insert(StyleProperty::OverflowWrap(parley_style.overflow_wrap));
        styles.insert(StyleProperty::TextWrapMode(parley_style.text_wrap_mode));
        styles.insert(StyleProperty::WhiteSpaceCollapse(
            parley_style.white_space_collapse,
        ));
        styles.insert(StyleProperty::Brush(parley_style.brush));
        editor.set_alignment(alignment);
        editor.set_base_direction(base_direction);

        editor.refresh_layout(&mut cx.font_ctx.lock().unwrap(), &mut cx.layout_ctx);
    }

    fn set_scale(&mut self, cx: &mut TextContext, scale: f32) {
        self.editor.set_scale(scale);
        let mut font_ctx = cx.font_ctx.lock().unwrap();
        self.editor
            .refresh_layout(&mut font_ctx, &mut cx.layout_ctx);
    }

    fn set_width(&mut self, cx: &mut TextContext, width: Option<f32>) {
        self.editor.set_width(width);
        self.editor
            .refresh_layout(&mut cx.font_ctx.lock().unwrap(), &mut cx.layout_ctx);
    }

    fn edit(&mut self, cx: &mut TextContext, edit: Edit<'_>) {
        let mut font_ctx = cx.font_ctx.lock().unwrap();
        let mut driver = self.editor.driver(&mut font_ctx, &mut cx.layout_ctx);
        match edit {
            Edit::Insert(text) => driver.insert_or_replace_selection(text),
            Edit::Delete => driver.delete(),
            Edit::Backdelete => driver.backdelete(),
            Edit::DeleteWord => driver.delete_word(),
            Edit::BackdeleteWord => driver.backdelete_word(),
            Edit::DeleteSelection => driver.delete_selection(),
            Edit::Move(motion) => match motion {
                Motion::Left => driver.move_left(),
                Motion::Right => driver.move_right(),
                Motion::WordLeft => driver.move_word_left(),
                Motion::WordRight => driver.move_word_right(),
                Motion::Up => driver.move_up(),
                Motion::Down => driver.move_down(),
                Motion::LineStart => driver.move_to_line_start(),
                Motion::LineEnd => driver.move_to_line_end(),
                Motion::HardLineStart => driver.move_to_hard_line_start(),
                Motion::HardLineEnd => driver.move_to_hard_line_end(),
                Motion::TextStart => driver.move_to_text_start(),
                Motion::TextEnd => driver.move_to_text_end(),
            },
            Edit::Extend(motion) => match motion {
                Motion::Left => driver.select_left(),
                Motion::Right => driver.select_right(),
                Motion::WordLeft => driver.select_word_left(),
                Motion::WordRight => driver.select_word_right(),
                Motion::Up => driver.select_up(),
                Motion::Down => driver.select_down(),
                Motion::LineStart => driver.select_to_line_start(),
                Motion::LineEnd => driver.select_to_line_end(),
                Motion::HardLineStart => driver.select_to_hard_line_start(),
                Motion::HardLineEnd => driver.select_to_hard_line_end(),
                Motion::TextStart => driver.select_to_text_start(),
                Motion::TextEnd => driver.select_to_text_end(),
            },
            Edit::SelectAll => driver.select_all(),
            Edit::CollapseSelection => driver.collapse_selection(),
            Edit::SelectByteRange(start, end) => driver.select_byte_range(start, end),
            Edit::SetCompose(text, cursor) => driver.set_compose(text, cursor),
            Edit::ClearCompose => driver.clear_compose(),
            Edit::MoveToPoint(x, y) => driver.move_to_point(x, y),
            Edit::ExtendSelectionToPoint(x, y) => driver.extend_selection_to_point(x, y),
            Edit::ShiftClickExtension(x, y) => driver.shift_click_extension(x, y),
            Edit::SelectWordAtPoint(x, y) => driver.select_word_at_point(x, y),
            Edit::SelectHardLineAtPoint(x, y) => driver.select_hard_line_at_point(x, y),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::node::TextInputData;
    use crate::text::parley::TextContext;
    use crate::text::{DocumentText as _, Edit, EditEngine as _, EditableText as _, Motion};

    /// Build a [`TextInputData`] with the given text laid out at scale 1.0.
    fn make_input(cx: &mut TextContext, is_multiline: bool, text: &str) -> TextInputData {
        let mut data = TextInputData::new(is_multiline);
        data.editor.editor.set_scale(1.0);
        data.editor.editor.set_text(text);
        data.editor
            .editor
            .driver(&mut cx.font_ctx.lock().unwrap(), &mut cx.layout_ctx)
            .refresh_layout();
        data
    }

    fn context() -> TextContext {
        TextContext::new(Some(parley::FontContext::new()))
    }

    #[test]
    fn short_text_does_not_scroll() {
        let mut cx = context();
        let mut data = make_input(&mut cx, false, "hi");
        // A wide content box that comfortably fits the text.
        data.clamp_scroll_offset(1000.0, 100.0);
        assert_eq!(data.scroll_offset, 0.0);
    }

    #[test]
    fn single_line_scrolls_to_follow_caret() {
        let mut cx = context();
        let text = "the quick brown fox jumps over the lazy dog repeatedly and at length";
        let mut data = make_input(&mut cx, false, text);
        let content_box_width = 40.0;
        let content_box_height = 20.0;

        // Caret at the end of a string that overflows a narrow input should scroll right.
        data.editor.edit(&mut cx, Edit::Move(Motion::TextEnd));
        data.clamp_scroll_offset(content_box_width, content_box_height);

        let layout_width = data.editor.size().unwrap().width as f32;
        if layout_width > content_box_width {
            assert!(
                data.scroll_offset > 0.0,
                "expected horizontal scroll for overflowing single-line input"
            );
            // The caret must be within the visible region after scrolling.
            let caret = data.editor.caret_rect().unwrap();
            assert!(caret.x1 as f32 <= data.scroll_offset + content_box_width + 0.5);
            assert!(caret.x0 as f32 >= data.scroll_offset - 0.5);
        }

        // Moving the caret back to the start should reset the scroll offset.
        data.editor.edit(&mut cx, Edit::Move(Motion::TextStart));
        data.clamp_scroll_offset(content_box_width, content_box_height);
        assert_eq!(data.scroll_offset, 0.0);
    }

    #[test]
    fn multiline_scrolls_vertically_not_horizontally() {
        let mut cx = context();
        let text = (0..40)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut data = make_input(&mut cx, true, &text);
        // Constrain the width so wrapping is well-defined.
        data.editor.set_width(&mut cx, Some(200.0));

        let content_box_width = 200.0;
        let content_box_height = 30.0;

        data.editor.edit(&mut cx, Edit::Move(Motion::TextEnd));
        data.clamp_scroll_offset(content_box_width, content_box_height);

        let layout_height = data.editor.size().unwrap().height as f32;
        if layout_height > content_box_height {
            assert!(
                data.scroll_offset > 0.0,
                "expected vertical scroll for overflowing multi-line input"
            );
        }
    }

    #[test]
    fn scroll_by_clamps_and_bubbles() {
        let mut cx = context();
        let text = (0..40)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut data = make_input(&mut cx, true, &text);
        data.editor.set_width(&mut cx, Some(200.0));

        let content_box_width = 200.0;
        let content_box_height = 30.0;
        let max = data.max_scroll_offset(content_box_width, content_box_height);
        assert!(max > 0.0, "test text should overflow the content box");

        // Scrolling up (positive delta decreases offset) while already at the top is a no-op and
        // the whole delta bubbles.
        assert_eq!(data.scroll_offset, 0.0);
        let bubbled = data.scroll_by(15.0, content_box_width, content_box_height);
        assert_eq!(data.scroll_offset, 0.0);
        assert_eq!(bubbled, 15.0);

        // Scrolling down moves the offset and consumes the delta.
        let bubbled = data.scroll_by(-10.0, content_box_width, content_box_height);
        assert_eq!(data.scroll_offset, 10.0);
        assert_eq!(bubbled, 0.0);

        // Scrolling past the end clamps to the maximum and bubbles the remainder. Starting at
        // offset 10 with max headroom of `max - 10`, a delta of `-(max + 100)` consumes
        // `max - 10` and bubbles the rest (`-110`).
        let bubbled = data.scroll_by(-(max + 100.0), content_box_width, content_box_height);
        assert_eq!(data.scroll_offset, max);
        assert!((bubbled - (-110.0)).abs() < 1e-3);
    }

    #[test]
    fn single_line_does_not_scroll_when_text_fits() {
        let mut cx = context();
        let mut data = make_input(&mut cx, false, "hi");
        // Wide content box; nothing to scroll, so all delta bubbles.
        let bubbled = data.scroll_by(-50.0, 1000.0, 100.0);
        assert_eq!(data.scroll_offset, 0.0);
        assert_eq!(bubbled, -50.0);
    }
}
