use crate::tui::theme::Theme;
use ratatui::{
    prelude::*,
    widgets::*,
};

pub(crate) fn label_and_text<'a>(
    label: impl ToString,
    content: impl ToString,
    label_width: usize,
    selected: bool,
    theme: &Theme,
) -> Paragraph<'a> {
    let label = format!("{:width$}", label.to_string(), width = label_width);
    let content = content.to_string();

    Paragraph::new(Line::from(
        [
            Span::raw(label),
            Span::styled(
                content,
                if selected {
                    theme.text_selected
                } else {
                    theme.text_default
                },
            ),
        ]
        .to_vec(),
    ))
}

pub(crate) fn label_and_bool<'a>(
    label: impl ToString,
    content: bool,
    label_width: usize,
    selected: bool,
    theme: &Theme,
) -> Paragraph<'a> {
    let content = if content { "[x]" } else { "[ ]" };
    label_and_text(label, content, label_width, selected, theme)
}
