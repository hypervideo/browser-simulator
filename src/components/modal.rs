use super::Component;
use crate::action::Action;
use crossterm::event::{
    Event,
    KeyCode,
    KeyEvent,
};
use eyre::Result;
use ratatui::{
    self,
    style::Color,
    widgets::{
        Block,
        Paragraph,
    },
};
use serde::{
    Deserialize,
    Serialize,
};
use strum::Display;
use tui_input::{
    backend::crossterm::EventHandler as _,
    Input,
};

#[derive(Display, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TextModalAction {
    ShowTextModal { title: String, content: String },
    TextModalCancel,
    TextModalSubmit(String),
}

#[derive(Debug)]
pub struct TextInputModal {
    input: Input,
    title: String,
}

impl TextInputModal {
    pub fn new(title: impl ToString, content: impl ToString) -> Self {
        let input = Input::new(content.to_string());
        debug!("TextInputModal new: {:?}", input);
        Self {
            input,
            title: title.to_string(),
        }
    }
}

impl Component for TextInputModal {
    fn draw(&mut self, frame: &mut ratatui::Frame<'_>, area: ratatui::prelude::Rect) -> Result<()> {
        // keep 2 for borders and 1 for cursor
        let width = area.width.max(3) - 3;
        let scroll = self.input.visual_scroll(width as usize);
        let style = Color::Yellow;
        let input = Paragraph::new(self.input.value())
            .style(style)
            .scroll((0, scroll as u16))
            .block(Block::bordered().title(self.title.clone()));

        let popup_area = layout::render_popup(input, frame);

        // Ratatui hides the cursor unless it's explicitly set. Position the  cursor past the
        // end of the input text and one line down from the border to the input line
        let x = self.input.visual_cursor().max(scroll) - scroll + 1;
        frame.set_cursor_position((popup_area.x + area.x + x as u16, popup_area.y + area.y + 1));

        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        let _state = self.input.handle_event(&Event::Key(key));

        let action = match key.code {
            KeyCode::Enter => Some(Action::TextModal(TextModalAction::TextModalSubmit(
                self.input.value().to_string(),
            ))),

            KeyCode::Esc => {
                if self.input.value().is_empty() {
                    Some(Action::TextModal(TextModalAction::TextModalCancel))
                } else {
                    self.input.reset();
                    None
                }
            }

            _ => None,
        };

        Ok(action)
    }
}

mod layout {

    use ratatui::{
        layout::{
            Constraint,
            Flex,
            Layout,
            Rect,
        },
        widgets::{
            Clear,
            Widget,
        },
        Frame,
    };

    /// Centers a [`Rect`] within another [`Rect`] using the provided [`Constraint`]s.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use ratatui::layout::{Constraint, Rect};
    ///
    /// let area = Rect::new(0, 0, 100, 100);
    /// let horizontal = Constraint::Percentage(20);
    /// let vertical = Constraint::Percentage(30);
    ///
    /// let centered = center(area, horizontal, vertical);
    /// ```
    pub fn center(area: Rect, horizontal: Constraint, vertical: Constraint) -> Rect {
        let [area] = Layout::horizontal([horizontal]).flex(Flex::Center).areas(area);
        let [area] = Layout::vertical([vertical]).flex(Flex::Center).areas(area);
        area
    }

    pub fn render_popup(popup: impl Widget, frame: &mut Frame) -> Rect {
        let area = center(
            frame.area(),
            Constraint::Max(120),
            Constraint::Length(3), // top and bottom border + content
        );
        frame.render_widget(Clear, area);
        frame.render_widget(popup, area);
        area
    }
}
