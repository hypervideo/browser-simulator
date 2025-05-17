use super::{
    modal::TextModalAction,
    Component,
};
use crate::{
    action::Action,
    browser::participant::ParticipantStore,
    config::Config,
};
use color_eyre::Result;
use crossterm::event::KeyCode;
use ratatui::{
    prelude::*,
    widgets::*,
};
use strum::Display;
use tokio::sync::mpsc::UnboundedSender;

#[derive(Debug)]
struct EditingState {
    field: SelectedField,
}

impl EditingState {
    fn title(&self) -> &'static str {
        match self.field {
            SelectedField::Url => "Edit URL",
            SelectedField::FakeVideoFile => "Edit Fake Video File", // new
            SelectedField::Start | SelectedField::FakeMedia | SelectedField::FakeVideo | SelectedField::Headless => "",
        }
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
enum SelectedField {
    #[default]
    Url,
    FakeMedia,
    FakeVideo,
    FakeVideoFile,
    Headless,
    Start,
}

#[derive(Debug, Clone, PartialEq, Eq, Display, serde::Serialize, serde::Deserialize)]
pub(crate) enum BrowserStartAction {
    MoveUp,
    MoveDown,
    StartEdit,
    StartBrowser,
    ToggleFakeMedia,
    ToggleFakeVideo,
    ToggleHeadless,
    DeleteSelectedField,
}

// -=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-

#[derive(Debug)]
pub struct BrowserStart {
    command_tx: Option<UnboundedSender<Action>>,
    config: Config,
    selected: SelectedField,
    editing: Option<EditingState>,
    suspended: bool,
    participant_store: ParticipantStore,
}

impl BrowserStart {
    pub fn new(participant_store: ParticipantStore) -> Self {
        Self {
            command_tx: None,
            config: Config::default(),
            selected: SelectedField::Url,
            editing: None,
            suspended: false,
            participant_store,
        }
    }
}

impl Component for BrowserStart {
    fn suspend(&mut self) -> Result<()> {
        self.suspended = true;
        Ok(())
    }
    fn resume(&mut self) -> Result<()> {
        self.suspended = false;
        Ok(())
    }
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> Result<()> {
        self.command_tx = Some(tx);
        Ok(())
    }

    fn register_config_handler(&mut self, config: Config) -> Result<()> {
        self.config = config;
        Ok(())
    }

    fn handle_key_event(&mut self, key: crossterm::event::KeyEvent) -> Result<Option<Action>> {
        if self.suspended {
            return Ok(None);
        }

        let editing = self.editing.is_some();
        let action = match key.code {
            KeyCode::Delete | KeyCode::Backspace if !editing => Some(BrowserStartAction::DeleteSelectedField),

            // navigation
            KeyCode::Up if !editing => Some(BrowserStartAction::MoveUp),
            KeyCode::Down if !editing => Some(BrowserStartAction::MoveDown),

            // start editing or start browser or toggle
            KeyCode::Enter if !editing && self.selected == SelectedField::Start => {
                Some(BrowserStartAction::StartBrowser)
            }
            KeyCode::Enter if !editing && self.selected == SelectedField::FakeMedia => {
                Some(BrowserStartAction::ToggleFakeMedia)
            }
            KeyCode::Enter if !editing && self.selected == SelectedField::FakeVideo => {
                Some(BrowserStartAction::ToggleFakeVideo)
            }
            KeyCode::Enter if !editing && self.selected == SelectedField::Headless => {
                Some(BrowserStartAction::ToggleHeadless)
            }
            KeyCode::Enter
                if !editing && matches!(self.selected, SelectedField::Url | SelectedField::FakeVideoFile) =>
            {
                Some(BrowserStartAction::StartEdit)
            } // Edit URL/Cookie/FakeVideoFile

            _ => None,
        };

        Ok(action.map(Action::BrowserStartAction))
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        if self.suspended {
            return Ok(None);
        }

        let action = match action {
            Action::TextModal(TextModalAction::TextModalCancel) => {
                self.editing = None;
                return Ok(None);
            }

            Action::TextModal(TextModalAction::TextModalSubmit(content)) if self.editing.is_some() => {
                if let Some(edit) = self.editing.take() {
                    match edit.field {
                        SelectedField::Url => self.config.url = content,
                        SelectedField::FakeVideoFile => {
                            // Set to None if the buffer is empty or only whitespace
                            self.config.fake_video_file = if content.trim().is_empty() { None } else { Some(content) };
                        }
                        SelectedField::FakeMedia
                        | SelectedField::FakeVideo
                        | SelectedField::Headless
                        | SelectedField::Start => {}
                    }
                    // Save config immediately after edit confirmation
                    if let Err(e) = self.config.save() {
                        error!(?e, "Failed to save config after edit");
                    }
                }
                return Ok(None);
            }

            Action::BrowserStartAction(action) => action,

            _ => return Ok(None),
        };

        match action {
            BrowserStartAction::MoveUp => {
                self.selected = match self.selected {
                    SelectedField::FakeMedia => SelectedField::Url,
                    SelectedField::FakeVideo => SelectedField::FakeMedia,
                    SelectedField::FakeVideoFile => SelectedField::FakeVideo,
                    SelectedField::Headless => {
                        if self.config.fake_video_file.is_some() {
                            SelectedField::FakeVideoFile // Go to file input if visible
                        } else {
                            SelectedField::FakeVideo // Otherwise go to video checkbox
                        }
                    }
                    SelectedField::Start => SelectedField::Headless,
                    other => other, // Url stays Url
                };
            }

            BrowserStartAction::MoveDown => {
                self.selected = match self.selected {
                    SelectedField::Url => SelectedField::FakeMedia,
                    SelectedField::FakeMedia => SelectedField::FakeVideo,
                    SelectedField::FakeVideo => {
                        if self.config.fake_video_file.is_some() {
                            SelectedField::FakeVideoFile // Go to file input if visible
                        } else {
                            SelectedField::Headless // Otherwise skip to Start
                        }
                    }
                    SelectedField::FakeVideoFile => SelectedField::Headless,
                    SelectedField::Headless => SelectedField::Start,
                    other => other, // Start stays Start
                };
            }

            // Edit
            BrowserStartAction::StartEdit if self.editing.is_none() => {
                let content = match self.selected {
                    SelectedField::Url => self.config.url.clone(),
                    SelectedField::FakeVideoFile => self.config.fake_video_file.clone().unwrap_or_default(),
                    _ => String::new(),
                };

                let state = EditingState { field: self.selected };
                let action = Action::TextModal(TextModalAction::ShowTextModal {
                    title: state.title().to_string(),
                    content,
                });
                self.editing = Some(state);
                return Ok(Some(action));
            }

            BrowserStartAction::StartEdit => {
                return Ok(None);
            }

            BrowserStartAction::DeleteSelectedField => {
                match self.selected {
                    SelectedField::Url => self.config.url.clear(),
                    SelectedField::FakeMedia => self.config.fake_media = false,
                    SelectedField::FakeVideoFile => {
                        self.config.fake_video_file = None;
                        // Ensure selection moves if it was on FakeVideoFile
                        if self.selected == SelectedField::FakeVideoFile {
                            self.selected = SelectedField::FakeVideo;
                        }
                    }
                    _ => return Ok(None),
                }
                if let Err(e) = self.config.save() {
                    error!(?e, "Failed to save config after deleting cookie");
                    // TODO: inform the user via TUI state
                }
            }

            BrowserStartAction::ToggleFakeMedia => {
                self.config.fake_media = !self.config.fake_media;
                if let Err(e) = self.config.save() {
                    error!(?e, "Failed to save config after toggling fake media");
                }
            }

            BrowserStartAction::ToggleFakeVideo => {
                if self.config.fake_video_file.is_some() {
                    self.config.fake_video_file = None;
                    // If toggling off, ensure selection moves if it was on FakeVideoFile
                    if self.selected == SelectedField::FakeVideoFile {
                        self.selected = SelectedField::FakeVideo;
                    }
                } else {
                    // Default to empty string, user needs to edit it
                    self.config.fake_video_file = Some(String::new());
                }
                if let Err(e) = self.config.save() {
                    error!(?e, "Failed to save config after toggling fake video");
                }
            }

            BrowserStartAction::ToggleHeadless => {
                self.config.headless = !self.config.headless;
                if let Err(e) = self.config.save() {
                    error!(?e, "Failed to save config after toggling headless mode");
                }
            }

            // Start Browser / Playwright
            BrowserStartAction::StartBrowser => {
                if self.editing.is_some() {
                    return Ok(None);
                }

                if let Err(e) = self.participant_store.spawn(&self.config) {
                    error!(?e, "Failed to spawn participant");
                    return Ok(None);
                }
            }
        };

        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame<'_>, area: Rect) -> Result<()> {
        if self.suspended {
            return Ok(());
        }

        // Dynamically create constraints based on UI elements
        let mut constraints = vec![
            Constraint::Length(3), // URL
            Constraint::Length(3), // Fake-media checkbox
            Constraint::Length(3), // Fake-video checkbox
        ];
        if self.config.fake_video_file.is_some() {
            constraints.push(Constraint::Length(3)); // Fake-video-file editor
        }
        constraints.push(Constraint::Length(3)); // Headless checkbox
        constraints.push(Constraint::Length(1)); // Spacer before Start
        constraints.push(Constraint::Length(3)); // Start button

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints) // Use the dynamic constraints
            .split(area);

        let mut current_row_index = 0;

        // --- URL ---
        let url = self.config.url.clone();
        let url_widget = Paragraph::new(url)
            .block(Block::bordered().title("URL (<del> to clear)"))
            .style(if self.selected == SelectedField::Url {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            });
        frame.render_widget(url_widget, rows[current_row_index]);
        current_row_index += 1;

        // --- Fake Media Checkbox ---
        let fake_media_txt = format!("{} Use fake media", if self.config.fake_media { "[x]" } else { "[ ]" });
        let fake_media_widget = Paragraph::new(fake_media_txt).block(Block::bordered()).style(
            if self.selected == SelectedField::FakeMedia {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            },
        );
        frame.render_widget(fake_media_widget, rows[current_row_index]);
        current_row_index += 1;

        // --- Fake Video Checkbox ---
        let fake_video_txt = format!(
            "{} Enable fake video source",
            if self.config.fake_video_file.is_some() {
                "[x]"
            } else {
                "[ ]"
            }
        );
        let fake_video_widget = Paragraph::new(fake_video_txt).block(Block::bordered()).style(
            if self.selected == SelectedField::FakeVideo {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            },
        );
        frame.render_widget(fake_video_widget, rows[current_row_index]);
        current_row_index += 1;

        // --- Fake Video File Input (Conditional) ---
        if let Some(path) = &self.config.fake_video_file {
            let display = if path.is_empty() { "<empty>" } else { path };
            let vf_widget = Paragraph::new(display)
                .block(Block::bordered().title("Fake video file (<del> to clear)"))
                .style(if self.selected == SelectedField::FakeVideoFile {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default()
                });
            frame.render_widget(vf_widget, rows[current_row_index]);
            current_row_index += 1;
        }

        // --- Headless Checkbox ---
        let headless_txt = format!("{} Headless", if self.config.headless { "[x]" } else { "[ ]" });
        let headless_widget =
            Paragraph::new(headless_txt)
                .block(Block::bordered())
                .style(if self.selected == SelectedField::Headless {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default()
                });
        frame.render_widget(headless_widget, rows[current_row_index]);
        current_row_index += 1;

        // Skip the spacer row index
        current_row_index += 1;

        // --- Start Button ---
        let start_widget = Paragraph::new("Start Browser")
            .block(Block::bordered().border_style(Style::new().white()))
            .style(if self.selected == SelectedField::Start {
                Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow)
            } else {
                Style::default().add_modifier(Modifier::BOLD)
            });
        frame.render_widget(start_widget, rows[current_row_index]);

        Ok(())
    }
}
