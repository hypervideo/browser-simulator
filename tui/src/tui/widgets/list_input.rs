use crate::tui::theme::Theme;
use eyre::Result;
use ratatui::{
    prelude::*,
    widgets::{
        self,
        Block,
        Borders,
    },
};

#[derive(Debug)]
pub(crate) struct ListItem<T> {
    label: String,
    value: T,
}

impl<A, B, T> From<(A, B)> for ListItem<T>
where
    A: Into<String>,
    B: Into<T>,
{
    fn from(arg: (A, B)) -> Self {
        let (label, value) = arg;
        ListItem {
            label: label.into(),
            value: value.into(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ListInput<T> {
    title: &'static str,
    items: Vec<ListItem<T>>,
    list_state: widgets::ListState,
}

impl<T> ListInput<T> {
    pub(crate) fn new(
        title: &'static str,
        items: impl IntoIterator<Item = impl Into<ListItem<T>>>,
        selected: Option<usize>,
    ) -> Self {
        let items = items.into_iter().map(|ea| ea.into()).collect::<Vec<_>>();
        Self {
            title,
            items,
            list_state: widgets::ListState::default().with_selected(selected),
        }
    }

    pub(crate) fn draw(&mut self, frame: &mut ratatui::Frame<'_>, _area: ratatui::prelude::Rect) -> Result<()> {
        // Iterate through all elements in the `items` and stylize them.
        let items: Vec<widgets::ListItem> = self
            .items
            .iter()
            .map(|item| widgets::ListItem::new(item.label.as_str()))
            .collect();

        let line_count = self.items.len() as u16;

        let block = Block::default().borders(Borders::ALL).title(self.title);

        // Create a List from all list items and highlight the currently selected one
        let list = widgets::List::new(items)
            .block(block)
            .highlight_style(Theme::default().text_selected)
            .highlight_symbol("> ")
            .highlight_spacing(widgets::HighlightSpacing::Always);

        render_popup(list, frame, &mut self.list_state, line_count);

        Ok(())
    }

    pub(crate) fn handle_key_event(&mut self, key: crossterm::event::KeyEvent) -> bool {
        match key.code {
            crossterm::event::KeyCode::Up => {
                self.list_state.select_previous();
            }
            crossterm::event::KeyCode::Down => {
                self.list_state.select_next();
            }
            crossterm::event::KeyCode::Home => {
                self.list_state.select_first();
            }
            crossterm::event::KeyCode::End => {
                self.list_state.select_last();
            }
            _ => {
                return false;
            }
        }
        true
    }

    pub(crate) fn finish(mut self) -> Option<(usize, T)> {
        let index = self.list_state.selected()?;
        (index < self.items.len()).then(|| (index, self.items.remove(index).value))
    }
}

fn render_popup<T: StatefulWidget>(popup: T, frame: &mut Frame, state: &mut T::State, line_count: u16) -> Rect {
    let area = crate::tui::layout::center(frame.area(), Constraint::Max(120), Constraint::Length(line_count + 2));
    frame.render_stateful_widget(popup, area, state);
    area
}
