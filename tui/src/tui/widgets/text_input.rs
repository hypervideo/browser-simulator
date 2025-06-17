use crate::tui::layout;
use eyre::Result;
use ratatui::{
    self,
    layout::{
        Constraint,
        Rect,
    },
    style::Style,
    widgets::{
        Block,
        Borders,
        Clear,
        Widget,
    },
    Frame,
};
use tui_textarea::TextArea;

#[derive(Debug)]
pub(crate) struct TextInput {
    editor: TextArea<'static>,
}

impl TextInput {
    pub(crate) fn new(title: &'static str, placeholder: &'static str, content: impl ToString) -> Self {
        let mut editor = TextArea::new([content.to_string()].to_vec());
        editor.set_cursor_line_style(Style::default());
        editor.set_placeholder_text(placeholder);
        editor.set_block(Block::default().borders(Borders::ALL).title(title));
        editor.select_all();
        Self { editor }
    }

    pub(crate) fn draw(&mut self, frame: &mut ratatui::Frame<'_>, _area: ratatui::prelude::Rect) -> Result<()> {
        render_popup(&self.editor, frame);
        Ok(())
    }

    pub(crate) fn handle_key_event(&mut self, key: crossterm::event::KeyEvent) -> bool {
        self.editor.input(key)
    }

    pub(crate) fn finish(self) -> String {
        self.editor.into_lines().join("\n")
    }
}

fn render_popup(popup: impl Widget, frame: &mut Frame) -> Rect {
    let area = layout::center(
        frame.area(),
        Constraint::Max(120),
        Constraint::Length(3), // top and bottom border + content
    );
    frame.render_widget(Clear, area);
    frame.render_widget(popup, area);
    area
}
