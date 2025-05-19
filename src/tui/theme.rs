use ratatui::style::{
    Color,
    Style,
};

#[derive(Clone, Copy, Debug)]
pub(super) struct Theme {
    pub(super) default_style: Style,
    pub(super) border_focused: Style,
    pub(super) border_unfocused: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            default_style: Style::default().bg(Color::Black).fg(Color::Gray),
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
