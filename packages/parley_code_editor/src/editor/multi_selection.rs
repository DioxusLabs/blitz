//! Commands that add, remove or reshape selection regions

use floem_editor_core::{
    buffer::rope_text::RopeText,
    selection::{SelRegion, Selection},
    xi_rope::{
        Cursor, Rope,
        find::{CaseMatching, find},
    },
};

use super::{CodeEditor, Movement};
use crate::TextContext;

/// How many selections [`MultiSelectCommand::Undo`] can step back through
const SELECTION_HISTORY_LIMIT: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum MultiSelectCommand {
    /// Add a caret on the line above the topmost region
    CursorAbove,
    /// Add a caret on the line below the bottommost region
    CursorBelow,
    /// Extend each region to whole lines, or by one more line if it already covers whole lines
    SelectLine,
    /// Select the word at the caret, or add the next occurrence of the primary selection
    SelectNextOccurrence,
    /// Select every occurrence of the primary selection (or the word at the caret)
    SelectAllOccurrences,
    /// Go back to the selection before the last multi-selection command
    Undo,
}

/// The start offset of the first match of `pat` at or after `from`
fn find_from(text: &Rope, pat: &str, from: usize) -> Option<usize> {
    let mut cursor = Cursor::new(text, from);
    let mut lines = text.lines_raw(from..text.len());
    find(&mut cursor, &mut lines, CaseMatching::Exact, pat, None)
}

impl CodeEditor {
    pub(super) fn multi_select(&mut self, cx: &mut TextContext, cmd: MultiSelectCommand) {
        let selection = self.selection();
        let new_selection = match cmd {
            MultiSelectCommand::CursorAbove => self.add_cursor_vertically(cx, &selection, true),
            MultiSelectCommand::CursorBelow => self.add_cursor_vertically(cx, &selection, false),
            MultiSelectCommand::SelectLine => self.select_lines(&selection),
            MultiSelectCommand::SelectNextOccurrence => self.select_next_occurrence(&selection),
            MultiSelectCommand::SelectAllOccurrences => self.select_all_occurrences(&selection),
            MultiSelectCommand::Undo => {
                if let Some(previous) = self.cursor.history_selections.pop() {
                    self.set_selection(previous);
                }
                return;
            }
        };
        let Some(new_selection) = new_selection.filter(|s| *s != selection) else {
            return;
        };

        let history = &mut self.cursor.history_selections;
        history.push(selection);
        if history.len() > SELECTION_HISTORY_LIMIT {
            history.remove(0);
        }
        self.set_selection(new_selection);
    }

    fn add_cursor_vertically(
        &mut self,
        cx: &mut TextContext,
        selection: &Selection,
        above: bool,
    ) -> Option<Selection> {
        let edge = if above {
            selection.first()
        } else {
            selection.last()
        };
        let edge = *edge?;
        // Up/Down on the first/last row move to the start/end of the document instead
        let row = self.offset_row(cx, edge.end);
        if (above && row == 0) || (!above && row + 1 >= self.cache.rows().total()) {
            return None;
        }
        let caret = SelRegion::new(edge.end, edge.end, edge.affinity, edge.horiz);
        let movement = if above { Movement::Up } else { Movement::Down };
        let moved = self.move_region(cx, &caret, movement, false);
        let mut selection = selection.clone();
        selection.add_region(moved);
        Some(selection)
    }

    fn select_lines(&self, selection: &Selection) -> Option<Selection> {
        let mut new_selection = Selection::new();
        for region in selection.regions() {
            let start = self
                .buffer
                .offset_of_line(self.buffer.line_of_offset(region.min()));
            // A region that already covers whole lines ends at the start of the next line, so
            // this extends it by one more line
            let end = self
                .buffer
                .offset_of_line(self.buffer.line_of_offset(region.max()) + 1);
            new_selection.add_region(SelRegion::new(start, end, region.affinity, None));
        }
        Some(new_selection)
    }

    /// The text to search for: the primary region's text, or `None` if the primary region is
    /// a caret (in which case the word at the caret is selected first)
    fn search_text(&self, selection: &Selection) -> Option<String> {
        let primary = selection.last_inserted()?;
        (!primary.is_caret()).then(|| {
            self.buffer
                .slice_to_cow(primary.min()..primary.max())
                .into_owned()
        })
    }

    /// Selects the word at each caret
    fn select_words(&self, selection: &Selection) -> Option<Selection> {
        let mut new_selection = Selection::new();
        for region in selection.regions() {
            let (start, end) = if region.is_caret() {
                self.buffer.select_word(region.end)
            } else {
                (region.start, region.end)
            };
            new_selection.add_region(SelRegion::new(start, end, region.affinity, None));
        }
        Some(new_selection)
    }

    fn select_next_occurrence(&self, selection: &Selection) -> Option<Selection> {
        let Some(needle) = self.search_text(selection) else {
            return self.select_words(selection);
        };
        let primary = *selection.last_inserted()?;
        let text = self.buffer.text();
        let is_selected = |start: usize| {
            selection
                .regions()
                .iter()
                .any(|r| r.min() == start && r.max() == start + needle.len())
        };

        // Search forward from the primary region, wrapping around to the start once
        let mut from = primary.max();
        let mut wrapped = false;
        loop {
            match find_from(text, &needle, from) {
                Some(start) if wrapped && start >= primary.max() => return None,
                Some(start) if is_selected(start) => from = start + needle.len(),
                Some(start) => {
                    let mut selection = selection.clone();
                    let (anchor, focus) = if primary.start <= primary.end {
                        (start, start + needle.len())
                    } else {
                        (start + needle.len(), start)
                    };
                    selection.add_region(SelRegion::new(anchor, focus, primary.affinity, None));
                    return Some(selection);
                }
                None if !wrapped => {
                    wrapped = true;
                    from = 0;
                }
                None => return None,
            }
        }
    }

    fn select_all_occurrences(&self, selection: &Selection) -> Option<Selection> {
        let needle = match self.search_text(selection) {
            Some(needle) => needle,
            None => {
                let primary = selection.last_inserted()?;
                let (start, end) = self.buffer.select_word(primary.end);
                self.buffer.slice_to_cow(start..end).into_owned()
            }
        };
        if needle.is_empty() {
            return None;
        }
        let affinity = selection.get_cursor_affinity();
        let text = self.buffer.text();
        let mut new_selection = Selection::new();
        let mut from = 0;
        while let Some(start) = find_from(text, &needle, from) {
            let end = start + needle.len();
            new_selection.add_region(SelRegion::new(start, end, affinity, None));
            from = end;
        }
        Some(new_selection)
    }
}
