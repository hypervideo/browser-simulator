use crate::{
    browser::participant::ParticipantStore,
    config::{
        Keymap,
        NoiseSuppression,
        WebcamResolution,
    },
    tui::{
        layout::header_and_two_main_areas,
        widgets::EnumListInput,
        Action,
        ActivateAction,
        Component,
        FocusedTopLevelComponent,
        Theme,
    },
};
use chrono::TimeDelta;
use color_eyre::Result;
use crossterm::event::KeyCode;
use eyre::OptionExt as _;
use ratatui::{
    layout::{
        Constraint,
        Rect,
    },
    style::{
        Color,
        Style,
    },
    text::Line,
    widgets::{
        Cell,
        Row,
        Table,
        TableState,
    },
    Frame,
};
use strum::{
    Display,
    IntoEnumIterator as _,
};

#[derive(Debug, Clone, PartialEq, Eq, Display, serde::Serialize, serde::Deserialize)]
pub(crate) enum ParticipantsAction {
    MoveUp,
    MoveDown,
    StartSelectNoiseSuppression,
    StartSelectResolution,
}

#[derive(Debug)]
pub struct Participants {
    focused: bool,
    visible: bool,
    participants: ParticipantStore,
    selected: Option<String>,
    table_state: TableState,
    keymap: Keymap,
    noise_suppression_list: Option<EnumListInput<NoiseSuppression>>,
    resolution_list: Option<EnumListInput<WebcamResolution>>,
}

impl Participants {
    pub(crate) fn new(participants: ParticipantStore) -> Self {
        Self {
            focused: false,
            visible: true,
            selected: None,
            participants,
            table_state: TableState::default(),
            keymap: Keymap::default(),
            noise_suppression_list: None,
            resolution_list: None,
        }
    }

    fn render_tick(&mut self) -> Result<()> {
        Ok(())
    }

    fn move_up(&mut self) {
        let keys = self.participants.keys();
        if let Some(key) = &self.selected {
            let index = keys.iter().position(|x| x == key);
            if let Some(index) = index {
                if index > 0 {
                    self.selected = keys.get(index - 1).cloned();
                } else {
                    self.selected = None;
                }
            }
        } else {
            self.selected = None;
        }
    }

    fn move_down(&mut self) {
        let keys = self.participants.keys();
        if let Some(key) = &self.selected {
            let index = keys.iter().position(|x| x == key);
            if let Some(index) = index {
                if index < keys.len() - 1 {
                    self.selected = keys.get(index + 1).cloned();
                }
            }
        } else {
            self.selected = self.participants.keys().first().cloned();
        }
    }
}

impl Component for Participants {
    fn is_visible(&self) -> bool {
        self.visible
    }

    fn is_focused(&self) -> bool {
        self.focused
    }

    fn register_config_handler(&mut self, config: crate::config::Config) -> Result<()> {
        self.keymap = config
            .keybindings
            .get(&FocusedTopLevelComponent::Participants)
            .cloned()
            .ok_or_eyre("No keymap found for Participants")?;
        Ok(())
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        match action {
            Action::Activate(ActivateAction::Participants) => {
                self.focused = true;
                self.visible = true;
                self.selected = self.participants.keys().first().cloned();
                return Ok(Some(Action::UpdateGlobalKeybindings(self.keymap.clone())));
            }
            Action::Activate(ActivateAction::BrowserStart) => {
                self.focused = false;
                self.visible = true;
                return Ok(None);
            }
            Action::Activate(_) => {
                self.focused = false;
                self.visible = false;
            }
            Action::Render => self.render_tick()?,
            Action::ParticipantsAction(inner) => match inner {
                ParticipantsAction::MoveUp => {
                    self.move_up();
                    if self.selected.is_none() {
                        return Ok(Some(Action::Activate(ActivateAction::BrowserStart)));
                    }
                }
                ParticipantsAction::MoveDown => self.move_down(),

                ParticipantsAction::StartSelectNoiseSuppression => {
                    if let Some(selected) = self.selected.as_ref().and_then(|s| self.participants.get(s)) {
                        self.noise_suppression_list = Some(EnumListInput::new(
                            "Noise Suppression Models",
                            NoiseSuppression::iter(),
                            selected.state.borrow().noise_suppression,
                        ));
                    }
                    return Ok(None);
                }
                ParticipantsAction::StartSelectResolution => {
                    if let Some(selected) = self.selected.as_ref().and_then(|s| self.participants.get(s)) {
                        self.resolution_list = Some(EnumListInput::new(
                            "Camera resolution",
                            WebcamResolution::iter(),
                            selected.state.borrow().webcam_resolution,
                        ));
                    }
                    return Ok(None);
                }
            },
            _ => {}
        }
        Ok(None)
    }

    fn handle_key_event(&mut self, key: crossterm::event::KeyEvent) -> Result<Option<Action>> {
        if let Some(mut list) = self.noise_suppression_list.take() {
            match key.code {
                KeyCode::Enter => {
                    if let Ok(value) = list.finish() {
                        if let Some(participant) = self.selected.as_ref().and_then(|s| self.participants.get(s)) {
                            participant.set_noise_suppression(value);
                        }
                    }
                    return Ok(Some(Action::Activate(ActivateAction::Participants)));
                }
                KeyCode::Esc => {
                    return Ok(Some(Action::Activate(ActivateAction::Participants)));
                }
                _ => {}
            }
            let handled = list.handle_key_event(key);
            self.noise_suppression_list = Some(list);
            if handled {
                return Ok(None);
            }
        }

        if let Some(mut list) = self.resolution_list.take() {
            match key.code {
                KeyCode::Enter => {
                    if let Ok(value) = list.finish() {
                        if let Some(participant) = self.selected.as_ref().and_then(|s| self.participants.get(s)) {
                            participant.set_webcam_resolutions(value);
                        }
                    }
                    return Ok(Some(Action::Activate(ActivateAction::Participants)));
                }
                KeyCode::Esc => {
                    return Ok(Some(Action::Activate(ActivateAction::BrowserStart)));
                }
                _ => {}
            }
            let handled = list.handle_key_event(key);
            self.resolution_list = Some(list);
            if handled {
                return Ok(None);
            }
        }

        let action = match (key.code, &self.selected) {
            (KeyCode::Backspace | KeyCode::Delete, Some(selected)) => {
                let prev = self.participants.prev(selected);
                if let Some(participant) = self.participants.get(selected) {
                    // We clone the store and move the participant and cloned store
                    // into a task that will wait until the participant closes the browser
                    // gracefully, and then we'll remove them from the store.
                    let store = self.participants.clone();
                    tokio::spawn(async move {
                        let name = participant.name.clone();
                        participant.close().await;
                        store.remove(&name);
                    });
                }
                self.selected = prev;
                Some(Action::ParticipantCountChanged(self.participants.len()))
            }

            (KeyCode::Char('l'), Some(selected)) => {
                if let Some(participant) = self.participants.get(selected) {
                    participant.leave();
                }

                None
            }

            (KeyCode::Char('j'), Some(selected)) => {
                if let Some(participant) = self.participants.get(selected) {
                    participant.join();
                }
                None
            }

            (KeyCode::Char('m'), Some(selected)) => {
                if let Some(participant) = self.participants.get(selected) {
                    participant.toggle_audio();
                }

                None
            }

            (KeyCode::Char('v'), Some(selected)) => {
                if let Some(participant) = self.participants.get(selected) {
                    participant.toggle_video();
                }
                None
            }

            (KeyCode::Char('n'), Some(_)) => Some(Action::ParticipantsAction(
                ParticipantsAction::StartSelectNoiseSuppression,
            )),

            (KeyCode::Char('r'), Some(_)) => {
                Some(Action::ParticipantsAction(ParticipantsAction::StartSelectResolution))
            }

            (KeyCode::Char('b'), Some(selected)) => {
                if let Some(participant) = self.participants.get(selected) {
                    participant.toggle_background_blur();
                }
                None
            }

            // navigation
            (KeyCode::Up, _) => Some(Action::ParticipantsAction(ParticipantsAction::MoveUp)),
            (KeyCode::Down, _) => Some(Action::ParticipantsAction(ParticipantsAction::MoveDown)),

            _ => None,
        };

        Ok(action)
    }

    fn draw(&mut self, frame: &mut Frame<'_>, area: Rect) -> Result<()> {
        let theme = Theme::default();
        let [_, _, area] = header_and_two_main_areas(area)?;

        let help = if self.selected.is_some() {
            " <del> to shutdown, <j>oin, <l>eave, <m>ute, <v>ideo, <n>oise suppression, <r>esolutions, <b>lur "
        } else {
            ""
        };

        let keys = self.participants.keys();

        if keys.is_empty() {
            let empty = ratatui::widgets::Block::default()
                .borders(ratatui::widgets::Borders::ALL)
                .border_style(theme.border(self.focused))
                .title("No participants");

            frame.render_widget(empty, area);
            return Ok(());
        }

        let header_names = [
            "Name",
            "Created",
            "Running",
            "Joined",
            "Muted",
            "Video active",
            "Noise Suppression",
            "Transport",
            "Resolution",
            "Blur",
        ];

        // Prepare table data
        let header_cells = header_names
            .iter()
            .map(|h| Cell::from(*h).style(Style::default().fg(Color::White)));
        let header = Row::new(header_cells)
            .style(Style::default().bg(Color::DarkGray).fg(Color::White))
            .height(1)
            .bottom_margin(0);

        let rows: Vec<Row> = self
            .participants
            .values()
            .iter()
            .map(|participant| {
                let created = format_duration(chrono::Utc::now() - participant.created);
                let state = participant.state.borrow();
                let opened = format_bool(state.running);
                let joined = format_bool(state.joined);
                let muted = format_bool(state.muted);
                let video = format_bool(state.video_activated);
                let noise_suppression = state.noise_suppression.to_string();
                let transport_mode = state.transport_mode.to_string();
                let resolution = state.webcam_resolution.to_string();
                let background_blur = format_bool(state.background_blur);
                let cells = vec![
                    Cell::from(participant.name.clone()),
                    Cell::from(created),
                    Cell::from(opened),
                    Cell::from(joined),
                    Cell::from(muted),
                    Cell::from(video),
                    Cell::from(noise_suppression),
                    Cell::from(transport_mode),
                    Cell::from(resolution),
                    Cell::from(background_blur),
                ];
                let style = if Some(&participant.name) == self.selected.as_ref() {
                    theme.text_selected
                } else {
                    theme.text_default
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
                    .border_style(theme.border(self.focused))
                    .title("Participants")
                    .title_bottom(Line::from(help).centered()),
            )
            .widths([
                Constraint::Percentage(10), // Name
                Constraint::Percentage(10), // Created
                Constraint::Percentage(10), // Running
                Constraint::Percentage(10), // Joined
                Constraint::Percentage(10), // Muted
                Constraint::Percentage(10), // Video active
                Constraint::Percentage(10), // Noise suppressed
                Constraint::Percentage(10), // Transport mode
                Constraint::Percentage(10), // Resolution
                Constraint::Percentage(10), // Blur
            ])
            .column_spacing(1);

        frame.render_stateful_widget(table, area, &mut self.table_state);

        // -=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-
        if let Some(list) = &mut self.noise_suppression_list {
            list.draw(frame, area)?;
        }
        if let Some(list) = &mut self.resolution_list {
            list.draw(frame, area)?;
        }

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

fn format_bool(value: bool) -> String {
    if value {
        "[x]".to_string()
    } else {
        "[ ]".to_string()
    }
}
