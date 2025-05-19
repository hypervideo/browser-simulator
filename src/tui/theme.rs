use ratatui::style::{
    Color,
    Style,
};

#[derive(Clone, Copy, Debug)]
pub(super) struct Theme {
    pub(super) default: Style,
    pub(super) text_default: Style,
    pub(super) text_selected: Style,
    pub(super) border_focused: Style,
    pub(super) border_unfocused: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            default: Style::default().bg(Color::Black).fg(Color::Gray),
            text_default: Style::default(),
            text_selected: Style::default().fg(Color::Yellow),
            border_focused: Style::default().fg(Color::White),
            border_unfocused: Style::default().fg(Color::DarkGray),
        }
    }
}

impl Theme {
    pub(super) fn border(&self, focused: bool) -> Style {
        if focused {
            self.border_focused
        } else {
            self.border_unfocused
        }
    }
}
