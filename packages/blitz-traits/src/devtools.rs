#[derive(Debug, Default, Clone, Copy)]
pub struct Devtools {
    pub show_layout: bool,
    pub highlight_hover: bool,
}

impl Devtools {
    pub fn toggle_show_layout(&mut self) {
        self.show_layout = !self.show_layout
    }

    pub fn toggle_highlight_hover(&mut self) {
        self.highlight_hover = !self.highlight_hover
    }
}
