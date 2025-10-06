//! Types configure developer inspection and debug tools

/// Configuration for debug overlays and other debugging tools
#[derive(Debug, Default, Clone, Copy)]
pub struct DevtoolSettings {
    /// Outline elements with different border colors depending on
    /// inner display style of that element
    pub show_layout: bool,
    /// Render browser-style colored overlay showing the content-box,
    /// padding, border, and margin of the hovered element
    pub highlight_hover: bool,
}

impl DevtoolSettings {
    /// Toggle the [`show_layout`](Self::show_layout) setting
    pub fn toggle_show_layout(&mut self) {
        self.show_layout = !self.show_layout
    }

    /// Toggle the [`highlight_hover`](Self::highlight_hover) setting
    pub fn toggle_highlight_hover(&mut self) {
        self.highlight_hover = !self.highlight_hover
    }
}
