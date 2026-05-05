use crate::tui::layout;
use crossterm::event::{
    KeyCode,
    KeyEvent,
    KeyModifiers,
};
use eyre::Result;
use ratatui::{
    self,
    layout::{
        Constraint,
        Rect,
    },
    style::{
        Color,
        Modifier,
        Style,
    },
    text::{
        Line,
        Span,
    },
    widgets::{
        Block,
        Borders,
        Clear,
        Paragraph,
    },
    Frame,
};

#[derive(Debug)]
pub(crate) struct TextInput {
    title: &'static str,
    placeholder: &'static str,
    content: String,
    cursor: usize,
    selected: bool,
}

impl TextInput {
    pub(crate) fn new(title: &'static str, placeholder: &'static str, content: impl ToString) -> Self {
        let content = content.to_string();
        let cursor = content.len();
        Self {
            title,
            placeholder,
            content,
            cursor,
            selected: true,
        }
    }

    pub(crate) fn draw(&mut self, frame: &mut ratatui::Frame<'_>, area: ratatui::prelude::Rect) -> Result<()> {
        let area = render_popup(frame, area);
        let block = Block::default().borders(Borders::ALL).title(self.title);
        let input_area = block.inner(area);
        frame.render_widget(block, area);

        let (line, cursor_offset) = self.visible_line(input_area.width);
        frame.render_widget(Paragraph::new(line), input_area);

        let cursor_x = input_area
            .x
            .saturating_add(cursor_offset)
            .min(input_area.right().saturating_sub(1));
        frame.set_cursor_position((cursor_x, input_area.y));
        Ok(())
    }

    pub(crate) fn handle_key_event(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char(ch) if is_text_input(key.modifiers) => {
                self.replace_selection();
                self.content.insert(self.cursor, ch);
                self.cursor += ch.len_utf8();
                true
            }
            KeyCode::Backspace => {
                if !self.replace_selection() {
                    self.delete_previous_char();
                }
                true
            }
            KeyCode::Delete => {
                if !self.replace_selection() {
                    self.delete_next_char();
                }
                true
            }
            KeyCode::Left => {
                if self.selected {
                    self.cursor = 0;
                    self.selected = false;
                } else {
                    self.cursor = previous_boundary(&self.content, self.cursor);
                }
                true
            }
            KeyCode::Right => {
                if self.selected {
                    self.cursor = self.content.len();
                    self.selected = false;
                } else {
                    self.cursor = next_boundary(&self.content, self.cursor);
                }
                true
            }
            KeyCode::Home => {
                self.cursor = 0;
                self.selected = false;
                true
            }
            KeyCode::End => {
                self.cursor = self.content.len();
                self.selected = false;
                true
            }
            _ => false,
        }
    }

    pub(crate) fn finish(self) -> String {
        self.content
    }

    fn replace_selection(&mut self) -> bool {
        if !self.selected {
            return false;
        }

        self.content.clear();
        self.cursor = 0;
        self.selected = false;
        true
    }

    fn delete_previous_char(&mut self) {
        if self.cursor == 0 {
            return;
        }

        let previous = previous_boundary(&self.content, self.cursor);
        self.content.replace_range(previous..self.cursor, "");
        self.cursor = previous;
    }

    fn delete_next_char(&mut self) {
        if self.cursor >= self.content.len() {
            return;
        }

        let next = next_boundary(&self.content, self.cursor);
        self.content.replace_range(self.cursor..next, "");
    }

    fn visible_line(&self, width: u16) -> (Line<'static>, u16) {
        if self.content.is_empty() {
            return (
                Line::from(Span::styled(
                    self.placeholder.to_string(),
                    Style::default().fg(Color::DarkGray),
                )),
                0,
            );
        }

        let chars = self.content.chars().collect::<Vec<_>>();
        let cursor = self.content[..self.cursor].chars().count();
        let width = usize::from(width).max(1);
        let start = if chars.len() <= width {
            0
        } else {
            cursor.saturating_sub(width.saturating_sub(1))
        };
        let end = (start + width).min(chars.len());
        let visible = chars[start..end].iter().collect::<String>();
        let cursor_offset = cursor.saturating_sub(start).min(width.saturating_sub(1)) as u16;

        if self.selected {
            (
                Line::from(Span::styled(visible, Style::default().add_modifier(Modifier::REVERSED))),
                cursor_offset,
            )
        } else {
            (Line::from(visible), cursor_offset)
        }
    }
}

fn render_popup(frame: &mut Frame, area: Rect) -> Rect {
    let area = layout::center(
        area,
        Constraint::Max(120),
        Constraint::Length(3), // top and bottom border + content
    );
    frame.render_widget(Clear, area);
    area
}

fn is_text_input(modifiers: KeyModifiers) -> bool {
    !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
}

fn previous_boundary(content: &str, cursor: usize) -> usize {
    content[..cursor]
        .char_indices()
        .next_back()
        .map_or(0, |(index, _)| index)
}

fn next_boundary(content: &str, cursor: usize) -> usize {
    content[cursor..]
        .char_indices()
        .nth(1)
        .map_or(content.len(), |(index, _)| cursor + index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn typing_replaces_initial_content() {
        let mut input = TextInput::new("Title", "Placeholder", "old");

        assert!(input.handle_key_event(key(KeyCode::Char('n'))));

        assert_eq!(input.finish(), "n");
    }

    #[test]
    fn navigation_and_delete_are_utf8_safe() {
        let mut input = TextInput::new("Title", "Placeholder", "");
        assert!(input.handle_key_event(key(KeyCode::Char('a'))));
        assert!(input.handle_key_event(key(KeyCode::Char('é'))));
        assert!(input.handle_key_event(key(KeyCode::Char('z'))));
        assert!(input.handle_key_event(key(KeyCode::Left)));
        assert!(input.handle_key_event(key(KeyCode::Backspace)));

        assert_eq!(input.finish(), "az");
    }
}
