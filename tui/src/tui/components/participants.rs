use crate::tui::{
    keybindings::{
        KeyBindings,
        Keymap,
    },
    layout::header_and_two_main_areas,
    widgets::EnumListInput,
    Action,
    ActivateAction,
    Component,
    FocusedTopLevelComponent,
    Theme,
};
use chrono::TimeDelta;
use client_simulator_browser::participant::ParticipantStore;
use client_simulator_config::{
    Config,
    NoiseSuppression,
    VideoConstraint,
    VideoMaxConcurrentTracksPreset,
};
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
    StartSelectVideoSetting,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Display, strum::EnumIter, strum::EnumString)]
enum VideoSetting {
    #[strum(to_string = "Outgoing constraint")]
    PublishWebcam,
    #[strum(to_string = "Incoming constraint")]
    Subscribe,
    #[strum(to_string = "Track limit")]
    MaxTracks,
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
    video_setting_menu: Option<EnumListInput<VideoSetting>>,
    video_constraint_publish_webcam_list: Option<EnumListInput<VideoConstraint>>,
    video_constraint_subscribe_list: Option<EnumListInput<VideoConstraint>>,
    video_max_concurrent_tracks_list: Option<EnumListInput<VideoMaxConcurrentTracksPreset>>,
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
            video_setting_menu: None,
            video_constraint_publish_webcam_list: None,
            video_constraint_subscribe_list: None,
            video_max_concurrent_tracks_list: None,
        }
    }

    fn render_tick(&mut self) -> Result<Option<Action>> {
        Ok(self.reconcile_selection())
    }

    fn reconcile_selection(&mut self) -> Option<Action> {
        let keys = self.participants.keys();

        if keys.is_empty() {
            self.selected = None;
            self.table_state.select(None);

            return self.focused.then_some(Action::Activate(ActivateAction::BrowserStart));
        }

        if self
            .selected
            .as_ref()
            .is_none_or(|selected| !keys.iter().any(|key| key == selected))
        {
            self.selected = keys.first().cloned();
        }

        let selected_index = self
            .selected
            .as_ref()
            .and_then(|selected| keys.iter().position(|key| key == selected));
        self.table_state.select(selected_index);

        None
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
            } else {
                self.selected = None;
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
            } else {
                self.selected = self.participants.keys().first().cloned();
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

    fn register_config_handler(&mut self, _config: Config, keybindings: KeyBindings) -> Result<()> {
        self.keymap = keybindings
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
            Action::Render => return self.render_tick(),
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
                ParticipantsAction::StartSelectVideoSetting => {
                    self.video_setting_menu = Some(EnumListInput::new(
                        "Video constraints",
                        VideoSetting::iter(),
                        VideoSetting::PublishWebcam,
                    ));
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

        if let Some(mut list) = self.video_setting_menu.take() {
            match key.code {
                KeyCode::Enter => {
                    if let Ok(value) = list.finish() {
                        if let Some(selected) = self.selected.as_ref().and_then(|s| self.participants.get(s)) {
                            let state = selected.state.borrow();
                            match value {
                                VideoSetting::PublishWebcam => {
                                    self.video_constraint_publish_webcam_list = Some(EnumListInput::new(
                                        "Outgoing webcam video constraint",
                                        VideoConstraint::iter(),
                                        state.video_constraint_publish_webcam,
                                    ));
                                }
                                VideoSetting::Subscribe => {
                                    self.video_constraint_subscribe_list = Some(EnumListInput::new(
                                        "Incoming video constraint",
                                        VideoConstraint::iter(),
                                        state.video_constraint_subscribe,
                                    ));
                                }
                                VideoSetting::MaxTracks => {
                                    self.video_max_concurrent_tracks_list = Some(EnumListInput::new(
                                        "Max concurrent webcam tracks",
                                        VideoMaxConcurrentTracksPreset::iter(),
                                        VideoMaxConcurrentTracksPreset::from_option(state.video_max_concurrent_tracks),
                                    ));
                                }
                            }
                        }
                    }
                    return Ok(None);
                }
                KeyCode::Esc => {
                    return Ok(Some(Action::Activate(ActivateAction::Participants)));
                }
                _ => {}
            }
            let handled = list.handle_key_event(key);
            self.video_setting_menu = Some(list);
            if handled {
                return Ok(None);
            }
        }

        if let Some(mut list) = self.video_constraint_publish_webcam_list.take() {
            match key.code {
                KeyCode::Enter => {
                    if let Ok(value) = list.finish() {
                        if let Some(participant) = self.selected.as_ref().and_then(|s| self.participants.get(s)) {
                            participant.set_video_constraint_publish_webcam(value);
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
            self.video_constraint_publish_webcam_list = Some(list);
            if handled {
                return Ok(None);
            }
        }

        if let Some(mut list) = self.video_constraint_subscribe_list.take() {
            match key.code {
                KeyCode::Enter => {
                    if let Ok(value) = list.finish() {
                        if let Some(participant) = self.selected.as_ref().and_then(|s| self.participants.get(s)) {
                            participant.set_video_constraint_subscribe(value);
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
            self.video_constraint_subscribe_list = Some(list);
            if handled {
                return Ok(None);
            }
        }

        if let Some(mut list) = self.video_max_concurrent_tracks_list.take() {
            match key.code {
                KeyCode::Enter => {
                    if let Ok(value) = list.finish() {
                        if let Some(participant) = self.selected.as_ref().and_then(|s| self.participants.get(s)) {
                            participant.set_video_max_concurrent_tracks(value.to_option());
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
            self.video_max_concurrent_tracks_list = Some(list);
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

            (KeyCode::Char('s'), Some(selected)) => {
                if let Some(participant) = self.participants.get(selected) {
                    participant.toggle_screen_share();
                }
                None
            }

            (KeyCode::Char('g'), Some(selected)) => {
                if let Some(participant) = self.participants.get(selected) {
                    participant.toggle_auto_gain_control();
                }
                None
            }

            (KeyCode::Char('n'), Some(_)) => Some(Action::ParticipantsAction(
                ParticipantsAction::StartSelectNoiseSuppression,
            )),

            (KeyCode::Char('r'), Some(_)) => {
                Some(Action::ParticipantsAction(ParticipantsAction::StartSelectVideoSetting))
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
            " <del> to shutdown, <j>oin, <l>eave, <m>ute, <v>ideo, <s>creenshare, auto <g>ain, <n>oise suppression, <r> video constraints, <b>lur "
        } else {
            ""
        };

        let keys = self.participants.keys();
        let participant_count = keys.len();

        if participant_count == 0 {
            let empty = ratatui::widgets::Paragraph::new("No participants").block(
                ratatui::widgets::Block::default()
                    .borders(ratatui::widgets::Borders::ALL)
                    .border_style(theme.border(self.focused))
                    .title(participants_panel_title(participant_count)),
            );

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
            "Screenshare",
            "Autogain",
            "Noise Suppression",
            "Transport",
            "Video constraints",
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
                let name = participant.name.clone();
                let opened = format_bool(state.running);
                let joined = format_bool(state.joined);
                let muted = format_bool(state.muted);
                let video = format_bool(state.video_activated);
                let screenshare = format_bool(state.screenshare_activated);
                let auto_gain_control = format_bool(state.auto_gain_control);
                let noise_suppression = state.noise_suppression.to_string();
                let transport_mode = state.transport_mode.to_string();
                let publish = state.video_constraint_publish_webcam.to_string();
                let subscribe = state.video_constraint_subscribe.to_string();
                let tracks = state
                    .video_max_concurrent_tracks
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "∞".to_string());
                let video_constraints = format!("out:{publish} in:{subscribe} t:{tracks}");
                let background_blur = format_bool(state.background_blur);
                let cells = vec![
                    Cell::from(name),
                    Cell::from(created),
                    Cell::from(opened),
                    Cell::from(joined),
                    Cell::from(muted),
                    Cell::from(video),
                    Cell::from(screenshare),
                    Cell::from(auto_gain_control),
                    Cell::from(noise_suppression),
                    Cell::from(transport_mode),
                    Cell::from(video_constraints),
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
                    .title(participants_panel_title(participant_count))
                    .title_bottom(Line::from(help).centered()),
            )
            .widths([
                Constraint::Percentage(9),  // Name
                Constraint::Percentage(7),  // Created
                Constraint::Percentage(6),  // Running
                Constraint::Percentage(6),  // Joined
                Constraint::Percentage(6),  // Muted
                Constraint::Percentage(7),  // Video active
                Constraint::Percentage(8),  // Screenshare active
                Constraint::Percentage(7),  // Auto gain
                Constraint::Percentage(11), // Noise suppression
                Constraint::Percentage(7),  // Transport mode
                Constraint::Percentage(18), // Video constraints
                Constraint::Percentage(8),  // Blur
            ])
            .column_spacing(1);

        frame.render_stateful_widget(table, area, &mut self.table_state);

        // -=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-
        if let Some(list) = &mut self.noise_suppression_list {
            list.draw(frame, area)?;
        }
        if let Some(list) = &mut self.video_setting_menu {
            list.draw(frame, area)?;
        }
        if let Some(list) = &mut self.video_constraint_publish_webcam_list {
            list.draw(frame, area)?;
        }
        if let Some(list) = &mut self.video_constraint_subscribe_list {
            list.draw(frame, area)?;
        }
        if let Some(list) = &mut self.video_max_concurrent_tracks_list {
            list.draw(frame, area)?;
        }

        Ok(())
    }
}

fn participants_panel_title(participant_count: usize) -> String {
    if participant_count == 0 {
        "Participants".to_string()
    } else {
        format!("Participants ({participant_count})")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        participants_panel_title,
        Participants,
    };
    use crate::tui::{
        Action,
        ActivateAction,
        Component,
    };
    use client_simulator_browser::participant::ParticipantStore;
    use client_simulator_config::Config;
    use std::{
        fs,
        path::PathBuf,
        time::{
            SystemTime,
            UNIX_EPOCH,
        },
    };
    use url::Url;

    #[tokio::test]
    async fn render_selects_first_remaining_participant_after_external_removal() {
        let store = participant_store();
        spawn_remote_participant(&store);
        spawn_remote_participant(&store);

        let keys = store.keys();
        assert_eq!(keys.len(), 2);

        let mut component = Participants::new(store.clone());
        component.selected = Some(keys[1].clone());
        component.focused = true;

        store.remove(&keys[1]);

        let action = component.update(Action::Render).expect("render update succeeds");

        assert_eq!(action, None);
        assert_eq!(component.selected, Some(keys[0].clone()));
    }

    #[tokio::test]
    async fn render_returns_browser_start_when_focused_list_becomes_empty() {
        let store = participant_store();
        spawn_remote_participant(&store);

        let key = store.keys().into_iter().next().expect("participant exists");

        let mut component = Participants::new(store.clone());
        component.selected = Some(key.clone());
        component.focused = true;

        store.remove(&key);

        let action = component.update(Action::Render).expect("render update succeeds");

        assert_eq!(action, Some(Action::Activate(ActivateAction::BrowserStart)));
        assert_eq!(component.selected, None);
    }

    #[test]
    fn participants_panel_title_omits_count_when_empty() {
        assert_eq!(participants_panel_title(0), "Participants");
    }

    #[test]
    fn participants_panel_title_includes_count_when_not_empty() {
        assert_eq!(participants_panel_title(5), "Participants (5)");
    }

    fn spawn_remote_participant(store: &ParticipantStore) {
        let mut config = Config::default();
        config.url = Some(Url::parse("https://example.com/room/demo").expect("valid url"));
        store.spawn_remote_stub(&config).expect("spawn remote stub participant");
    }

    fn participant_store() -> ParticipantStore {
        let data_dir = unique_test_data_dir();
        fs::create_dir_all(&data_dir).expect("create temp data dir");
        ParticipantStore::new(&data_dir)
    }

    fn unique_test_data_dir() -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("current time")
            .as_nanos();
        std::env::temp_dir().join(format!("hyper-browser-simulator-participants-test-{timestamp}"))
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
