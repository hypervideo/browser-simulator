use crate::tui::theme::Theme;
use client_simulator_config::OwnOrDefault;
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

/// A label and a switch that the participant either sets, as [`label_and_bool`] shows it, or leaves to hyper.video,
/// shown as `default`.
pub(crate) fn label_and_bool_setting<'a>(
    label: impl ToString,
    content: OwnOrDefault<bool>,
    label_width: usize,
    selected: bool,
    theme: &Theme,
) -> Paragraph<'a> {
    match content {
        OwnOrDefault::Default => label_and_text(label, content, label_width, selected, theme),
        OwnOrDefault::Own(content) => label_and_bool(label, content, label_width, selected, theme),
    }
}
