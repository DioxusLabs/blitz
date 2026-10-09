//! The text of an `<input>` or `<textarea>`, edited and laid out by Parley's `PlainEditor`.

use std::borrow::Cow;
use std::ops::Range;

use blitz_traits::node_id::NodeId;
use kurbo::{Rect, Size};
use parley::{PlainEditor, PlainEditorDriver, StyleProperty};
use style::properties::ComputedValues;

use super::{TextBrush, TextContext, style as stylo_to_parley};
use crate::text::{Edit, EditEngine, EditableText, EditorMetrics, Motion};

/// The text of an `<input>` or `<textarea>`, edited and laid out by Parley.
pub struct TextEditor {
    editor: PlainEditor<TextBrush>,
    /// The width lines wrap at, in device pixels, as last set.
    width: Option<f32>,
}

impl TextEditor {
    /// Parley's editor, which the renderer paints the text, the selection and the caret of.
    #[inline]
    pub fn plain_editor(&self) -> &PlainEditor<TextBrush> {
        &self.editor
    }
}

impl EditableText for TextEditor {
    #[inline]
    fn raw_text(&self) -> &str {
        self.editor.raw_text()
    }

    fn text(&self) -> Cow<'_, str> {
        if self.editor.is_composing() {
            Cow::Owned(self.editor.text().to_string())
        } else {
            Cow::Borrowed(self.editor.raw_text())
        }
    }

    #[inline]
    fn selection(&self) -> Range<usize> {
        self.editor.raw_selection().text_range()
    }

    #[inline]
    fn selected_text(&self) -> Option<&str> {
        self.editor.selected_text()
    }

    #[inline]
    fn is_selection_collapsed(&self) -> bool {
        self.editor.raw_selection().is_collapsed()
    }

    fn revealed_range(&self) -> Option<Range<usize>> {
        // Parley draws a password field's text as it is.
        None
    }

    fn metrics(&self) -> Option<EditorMetrics> {
        let layout = self.editor.try_layout()?;
        Some(EditorMetrics {
            size: Size::new(f64::from(layout.full_width()), f64::from(layout.height())),
            scale: layout.scale(),
        })
    }

    fn caret_rect(&self) -> Option<Rect> {
        self.editor.try_layout()?;
        let caret = self.editor.cursor_geometry(1.5)?;
        Some(Rect::new(caret.x0, caret.y0, caret.x1, caret.y1))
    }
}

impl EditEngine for TextEditor {
    fn new(_is_multiline: bool) -> Self {
        Self {
            editor: PlainEditor::new(16.0),
            width: None,
        }
    }

    fn set_initial_text(&mut self, text: &str) {
        self.editor.set_text(text);
    }

    fn set_text(&mut self, text: &str) {
        if self.editor.text() != text {
            self.editor.set_text(text);
        }
    }

    fn set_style(&mut self, node: NodeId, style: Option<&ComputedValues>, scale: f32) {
        let parley_style = style
            .map(|s| stylo_to_parley::style(node, s))
            .unwrap_or_default();
        let alignment = style
            .map(|s| stylo_to_parley::text_align(s.slow_clone_text_align()))
            .unwrap_or(parley::layout::Alignment::Start);
        let base_direction = style
            .map(|s| {
                stylo_to_parley::base_direction(
                    s.slow_clone_direction(),
                    s.slow_clone_unicode_bidi(),
                )
            })
            .unwrap_or(parley::BaseDirection::Auto);

        let editor = &mut self.editor;
        editor.set_scale(scale);
        editor.set_width(None);
        self.width = None;

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
    }

    fn set_scale(&mut self, scale: f32) {
        if self.editor.get_scale() != scale {
            self.editor.set_scale(scale);
        }
    }

    fn set_width(&mut self, width: Option<f32>) {
        if self.width != width {
            self.width = width;
            self.editor.set_width(width);
        }
    }

    fn refresh(&mut self, cx: &mut TextContext) {
        if self.editor.try_layout().is_none() {
            self.editor
                .refresh_layout(&mut cx.font_ctx.lock().unwrap(), &mut cx.layout_ctx);
        }
    }

    fn edit(&mut self, cx: &mut TextContext, edit: Edit<'_>) {
        let mut font_ctx = cx.font_ctx.lock().unwrap();
        let mut driver = self.editor.driver(&mut font_ctx, &mut cx.layout_ctx);
        driver.refresh_layout();
        match edit {
            // Parley masks nothing, so there is nothing to show in the clear.
            Edit::Insert(text) | Edit::InsertRevealed(text) => {
                driver.insert_or_replace_selection(text)
            }
            Edit::Conceal => {}
            Edit::Delete => driver.delete(),
            Edit::Backdelete => driver.backdelete(),
            Edit::DeleteWord => driver.delete_word(),
            Edit::BackdeleteWord => driver.backdelete_word(),
            Edit::DeleteSelection => driver.delete_selection(),
            Edit::Move(motion) => move_by(&mut driver, motion),
            Edit::Extend(motion) => extend_by(&mut driver, motion),
            Edit::SelectAll => driver.select_all(),
            Edit::CollapseSelection => driver.collapse_selection(),
            Edit::SelectByteRange(start, end) => driver.select_byte_range(start, end),
            Edit::Select(from, to) => {
                move_by(&mut driver, from);
                extend_by(&mut driver, to);
            }
            Edit::SetCompose(text, cursor) => driver.set_compose(text, cursor),
            Edit::ClearCompose => driver.clear_compose(),
            // PlainEditor keeps no history.
            Edit::Undo | Edit::Redo => {}
            Edit::MoveToPoint(x, y) => driver.move_to_point(x, y),
            Edit::ExtendSelectionToPoint(x, y) => driver.extend_selection_to_point(x, y),
            Edit::ShiftClickExtension(x, y) => driver.shift_click_extension(x, y),
            Edit::SelectWordAtPoint(x, y) => driver.select_word_at_point(x, y),
            Edit::SelectHardLineAtPoint(x, y) => driver.select_hard_line_at_point(x, y),
        }
        driver.refresh_layout();
    }
}

/// Moves the caret by `motion`, collapsing the selection.
fn move_by(driver: &mut PlainEditorDriver<'_, TextBrush>, motion: Motion) {
    match motion {
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
    }
}

/// Moves the selection's focus by `motion`, keeping its anchor.
fn extend_by(driver: &mut PlainEditorDriver<'_, TextBrush>, motion: Motion) {
    match motion {
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
    }
}

#[cfg(test)]
mod tests {
    use blitz_traits::node_id::NodeId;

    use super::TextEditor;
    use crate::node::TextInputData;
    use crate::text::parley::TextContext;
    use crate::text::{DocumentText as _, Edit, EditEngine, EditableText as _, Motion};

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

    /// The setters lay nothing out: the next edit or refresh does, once, and the caret follows
    /// what was set.
    #[test]
    fn setters_lay_out_on_the_next_edit_or_refresh() {
        let mut cx = context();
        let mut editor = <TextEditor as EditEngine>::new(false);
        editor.set_style(NodeId::from_u64(1), None, 1.0);
        editor.set_text("hello");
        assert_eq!(editor.metrics(), None);
        assert_eq!(editor.caret_rect(), None);

        editor.edit(&mut cx, Edit::Move(Motion::TextEnd));
        let metrics = editor.metrics().unwrap();
        assert_eq!(metrics.scale, 1.0);
        let caret = editor.caret_rect().unwrap();
        assert!(
            (caret.x0 - metrics.size.width).abs() < 2.0,
            "{caret:?} {metrics:?}"
        );

        editor.set_scale(2.0);
        assert_eq!(editor.metrics(), None);
        editor.refresh(&mut cx);
        let scaled = editor.metrics().unwrap();
        assert_eq!(scaled.scale, 2.0);
        assert!(
            scaled.size.width > metrics.size.width * 1.5,
            "{scaled:?} {metrics:?}"
        );
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

        let layout_width = data.editor.metrics().unwrap().size.width as f32;
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
        data.editor.set_width(Some(200.0));
        data.editor.refresh(&mut cx);

        let content_box_width = 200.0;
        let content_box_height = 30.0;

        data.editor.edit(&mut cx, Edit::Move(Motion::TextEnd));
        data.clamp_scroll_offset(content_box_width, content_box_height);

        let layout_height = data.editor.metrics().unwrap().size.height as f32;
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
        data.editor.set_width(Some(200.0));
        data.editor.refresh(&mut cx);

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
