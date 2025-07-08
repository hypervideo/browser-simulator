use crate::tui::theme::Theme;
use ratatui::widgets::*;

pub(crate) fn button<'a>(label: impl ToString, selected: bool, theme: &Theme) -> Paragraph<'a> {
    let label_text = label.to_string();

    Paragraph::new(label_text)
        .style(if selected {
            theme.border_focused
        } else {
            theme.border_unfocused
        })
        .block(
            Block::new()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(if selected {
                    theme.border_focused
                } else {
                    theme.border_unfocused
                })
                .padding(Padding::horizontal(1)),
        )
}
