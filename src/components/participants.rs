use super::Component;
use crate::{
    action::Action,
    browser::participant::ParticipantStore,
};
use chrono::TimeDelta;
use color_eyre::Result;
use crossterm::event::KeyCode;
use ratatui::{
    layout::{
        Constraint,
        Rect,
    },
    style::{
        Color,
        Style,
    },
    widgets::{
        Cell,
        List,
        ListItem,
        Row,
        Table,
        TableState,
    },
    Frame,
};
use std::sync::{
    atomic::{
        AtomicBool,
        Ordering,
    },
    Arc,
};
use strum::Display;

#[derive(Debug, Clone, PartialEq, Eq, Display, serde::Serialize, serde::Deserialize)]
pub(crate) enum ParticipantsAction {
    MoveUp,
    MoveDown,
}

#[derive(Debug, Clone)]
pub struct Participants {
    participants: ParticipantStore,
    selected: Option<String>,
    draw: bool,
    suspended: bool,
    table_state: TableState,
}

impl Participants {
    pub fn new(participants: ParticipantStore) -> Self {
        Self {
            selected: None,
            participants,
            draw: true,
            suspended: true,
            table_state: TableState::default(),
        }
    }
    fn render_tick(&mut self) -> Result<()> {
        Ok(())
    }
    pub fn len(&self) -> usize {
        self.participants.len()
    }
    pub fn move_up(&mut self) {
        let keys = self.participants.keys();
        if let Some(key) = &self.selected {
            let index = keys.iter().position(|x| x == key);
            if let Some(index) = index {
                if index > 0 {
                    self.selected = keys.get(index - 1).cloned();
                } else {
                    self.selected = keys.last().cloned();
                }
            }
        } else {
            self.selected = self.participants.keys().last().cloned();
        }
    }
    pub fn move_down(&mut self) {
        let keys = self.participants.keys();
        if let Some(key) = &self.selected {
            let index = keys.iter().position(|x| x == key);
            if let Some(index) = index {
                if index < keys.len() - 1 {
                    self.selected = keys.get(index + 1).cloned();
                } else {
                    self.selected = keys.first().cloned();
                }
            }
        } else {
            self.selected = self.participants.keys().first().cloned();
        }
    }
}

impl Component for Participants {
    fn suspend(&mut self) -> Result<()> {
        self.draw = false;
        self.suspended = true;
        Ok(())
    }
    fn resume(&mut self) -> Result<()> {
        self.draw = true;
        self.suspended = false;
        Ok(())
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        if self.suspended {
            return Ok(None);
        }

        match action {
            Action::Render => self.render_tick()?,
            Action::ParticipantsAction(inner) => match inner {
                ParticipantsAction::MoveUp => self.move_up(),
                ParticipantsAction::MoveDown => self.move_down(),
            },
            _ => {}
        }
        Ok(None)
    }

    fn handle_key_event(&mut self, key: crossterm::event::KeyEvent) -> Result<Option<Action>> {
        if self.suspended {
            return Ok(None);
        }

        let selected = self.selected.is_some();

        let action = match key.code {
            KeyCode::Char('x') | KeyCode::Delete if selected => {
                if let Some(participant) = self.participants.remove(&self.selected.clone().unwrap()) {
                    participant.close();
                    self.selected = None;
                }

                None
            }
            KeyCode::Char('l') if selected => {
                if let Some(participant) = self.participants.get(&self.selected.clone().unwrap()) {
                    participant.leave();
                }

                None
            }
            KeyCode::Char('j') if selected => {
                if let Some(participant) = self.participants.get(&self.selected.clone().unwrap()) {
                    participant.join();
                }

                None
            }
            KeyCode::Char('m') if selected => {
                if let Some(participant) = self.participants.get(&self.selected.clone().unwrap()) {
                    participant.toggle_audio();
                }

                None
            }
            KeyCode::Char('v') if selected => {
                if let Some(participant) = self.participants.get(&self.selected.clone().unwrap()) {
                    participant.toggle_video();
                }

                None
            }

            // navigation
            KeyCode::Up => Some(ParticipantsAction::MoveUp),
            KeyCode::Down => Some(ParticipantsAction::MoveDown),

            _ => None,
        };

        Ok(action.map(Action::ParticipantsAction))
    }

    fn draw(&mut self, frame: &mut Frame<'_>, area: Rect) -> Result<()> {
        if !self.draw {
            return Ok(());
        }

        let keys = self.participants.keys();

        if keys.is_empty() {
            let empty = List::new(vec![ListItem::new("No participants")]);
            frame.render_widget(empty, area);
            return Ok(());
        }

        let header_names = [
            "Name".to_string(),
            "Created".to_string(),
            "[x] Running".to_string(),
            "[j/l] Joined".to_string(),
            "[m] Muted".to_string(),
            "[v] Invisible".to_string(),
        ];

        // Prepare table data
        let header_cells = header_names
            .iter()
            .map(|h| Cell::from(h.clone()).style(Style::default().fg(Color::White)));
        let header = Row::new(header_cells)
            .style(Style::default().bg(Color::Gray).fg(Color::Black))
            .height(1)
            .bottom_margin(1);

        let rows: Vec<Row> = self
            .participants
            .values()
            .iter()
            .map(|participant| {
                let created = format_duration(chrono::Utc::now() - participant.created);
                let opened = format_atomic_bool(participant.running.clone());
                let joined = format_atomic_bool(participant.joined.clone());
                let muted = format_atomic_bool(participant.muted.clone());
                let invisible = format_atomic_bool(participant.invisible.clone());
                let cells = vec![
                    Cell::from(participant.name.clone()),
                    Cell::from(created),
                    Cell::from(opened),
                    Cell::from(joined),
                    Cell::from(muted),
                    Cell::from(invisible),
                ];
                let style = if Some(&participant.name) == self.selected.as_ref() {
                    Style::default().bg(Color::Cyan)
                } else {
                    Style::default()
                };
                Row::new(cells).style(style).height(1)
            })
            .collect();

        let widths = [Constraint::Length(5), Constraint::Length(5)];
        let table = Table::new(rows, widths)
            .header(header)
            .block(
                ratatui::widgets::Block::default()
                    .borders(ratatui::widgets::Borders::ALL)
                    .title("Participants"),
            )
            .widths([
                Constraint::Percentage(25), // Name
                Constraint::Percentage(25), // Created
                Constraint::Percentage(10), // Opened
                Constraint::Percentage(10), // Joined
                Constraint::Percentage(10), // Muted
                Constraint::Percentage(10), // Invisible
            ])
            .column_spacing(1);

        frame.render_stateful_widget(table, area, &mut self.table_state);
        Ok(())
    }
}

// Helper function to format duration
fn format_duration(value: TimeDelta) -> String {
    let seconds = value.as_seconds_f32().round() as i32;
    if seconds < 60 {
        format!("{}s ago", seconds)
    } else if seconds < 3600 {
        format!("{}m ago", seconds / 60)
    } else {
        format!("{}h ago", seconds / 3600)
    }
}

fn format_atomic_bool(value: Arc<AtomicBool>) -> String {
    if value.load(Ordering::Relaxed) {
        "[x]".to_string()
    } else {
        "[ ]".to_string()
    }
}
