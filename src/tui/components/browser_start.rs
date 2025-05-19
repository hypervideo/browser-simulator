use crate::{
    browser::participant::ParticipantStore,
    config::Config,
    tui::{
        layout::header_and_two_main_areas,
        widgets,
        Action,
        ActivateAction,
        Component,
        FocusedTopLevelComponent,
        Theme,
    },
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
    editor: widgets::TextInput,
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
enum SelectedField {
    #[default]
    Url,
    FakeMedia,
    FakeVideoFile,
    Headless,
    Start,
}

impl SelectedField {
    fn selected_help(&self) -> &'static str {
        match self {
            SelectedField::Url => " URL to a hyper.video session. <enter> to edit, <del> to clear. ",
            SelectedField::FakeMedia => {
                " Use a test video and audio stream instead of real media devices. <enter> to toggle. "
            }
            SelectedField::FakeVideoFile => " Use audio and video from a file. <enter> to edit, <del> to clear. ",
            SelectedField::Headless => " Run the browser in headless mode? <enter> to toggle. ",
            SelectedField::Start => " Start a new browser session and join a hyper.video session. <enter> to start. ",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Display, serde::Serialize, serde::Deserialize)]
pub(crate) enum BrowserStartAction {
    MoveUp,
    MoveDown,
    StartEdit,
    StartBrowser,
    ToggleFakeMedia,
    ToggleHeadless,
    DeleteSelectedField,
}

// -=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-

#[derive(Debug)]
pub struct BrowserStart {
    focused: bool,
    visible: bool,
    command_tx: Option<UnboundedSender<Action>>,
    config: Config,
    selected: SelectedField,
    editing: Option<EditingState>,
    participant_store: ParticipantStore,
}

impl BrowserStart {
    pub fn new(participant_store: ParticipantStore) -> Self {
        Self {
            focused: true,
            visible: true,
            command_tx: None,
            config: Config::default(),
            selected: SelectedField::Url,
            editing: None,
            participant_store,
        }
    }
}

impl Component for BrowserStart {
    fn is_visible(&self) -> bool {
        self.visible
    }

    fn is_focused(&self) -> bool {
        self.focused
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
        if let Some(mut editing) = self.editing.take() {
            match key.code {
                KeyCode::Enter => {
                    let content = editing.editor.finish();
                    match editing.field {
                        SelectedField::Url => self.config.url = content,
                        SelectedField::FakeVideoFile => {
                            // Set to None if the buffer is empty or only whitespace
                            self.config.fake_video_file = if content.trim().is_empty() { None } else { Some(content) };
                        }
                        SelectedField::FakeMedia | SelectedField::Headless | SelectedField::Start => {}
                    }
                    // Save config immediately after edit confirmation
                    if let Err(e) = self.config.save() {
                        error!(?e, "Failed to save config after edit");
                    }
                    return Ok(Some(Action::Activate(ActivateAction::BrowserStart)));
                }
                KeyCode::Esc => {
                    return Ok(Some(Action::Activate(ActivateAction::BrowserStart)));
                }
                _ => {}
            }

            let handled = editing.editor.handle_key_event(key);
            self.editing = Some(editing);
            if handled {
                return Ok(None);
            }
        }

        let action = match key.code {
            KeyCode::Delete | KeyCode::Backspace => Some(BrowserStartAction::DeleteSelectedField),

            // navigation
            KeyCode::Up => Some(BrowserStartAction::MoveUp),
            KeyCode::Down => Some(BrowserStartAction::MoveDown),

            // start editing or start browser or toggle
            KeyCode::Enter if self.selected == SelectedField::Start => Some(BrowserStartAction::StartBrowser),
            KeyCode::Enter if self.selected == SelectedField::FakeMedia => Some(BrowserStartAction::ToggleFakeMedia),
            KeyCode::Enter if self.selected == SelectedField::Headless => Some(BrowserStartAction::ToggleHeadless),
            KeyCode::Enter if matches!(self.selected, SelectedField::Url | SelectedField::FakeVideoFile) => {
                Some(BrowserStartAction::StartEdit)
            }

            _ => None,
        };

        Ok(action.map(Action::BrowserStartAction))
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        let action = match action {
            Action::Activate(ActivateAction::BrowserStart) => {
                self.focused = true;
                self.visible = true;
                return Ok(self
                    .config
                    .keybindings
                    .get(&FocusedTopLevelComponent::BrowserStart)
                    .cloned()
                    .map(Action::UpdateGlobalKeybindings));
            }
            Action::Activate(ActivateAction::Participants) => {
                self.focused = false;
                self.visible = true;
                return Ok(None);
            }
            Action::Activate(_) => {
                self.focused = false;
                self.visible = false;
                return Ok(None);
            }

            Action::BrowserStartAction(action) => action,

            _ => return Ok(None),
        };

        match action {
            BrowserStartAction::MoveUp => {
                self.selected = match self.selected {
                    SelectedField::FakeMedia => SelectedField::Url,
                    SelectedField::FakeVideoFile => SelectedField::FakeMedia,
                    SelectedField::Headless => SelectedField::FakeVideoFile,
                    SelectedField::Start => SelectedField::Headless,
                    other => other, // Url stays Url
                };
            }

            BrowserStartAction::MoveDown => {
                self.selected = match self.selected {
                    SelectedField::Url => SelectedField::FakeMedia,
                    SelectedField::FakeMedia => SelectedField::FakeVideoFile,
                    SelectedField::FakeVideoFile => SelectedField::Headless,
                    SelectedField::Headless => SelectedField::Start,
                    SelectedField::Start => return Ok(Some(Action::Activate(ActivateAction::Participants))),
                };
            }

            // Edit
            BrowserStartAction::StartEdit if self.editing.is_none() => {
                let (title, placeholder, content) = match self.selected {
                    SelectedField::Url => ("Edit URL", "URL to a hyper.video session", self.config.url.clone()),
                    SelectedField::FakeVideoFile => (
                        "Edit path to video file",
                        "",
                        self.config.fake_video_file.clone().unwrap_or_default(),
                    ),
                    _ => ("", "", String::new()),
                };

                let state = EditingState {
                    field: self.selected,
                    editor: widgets::TextInput::new(title, placeholder, content),
                };
                self.editing = Some(state);
                return Ok(Some(Action::UpdateGlobalKeybindings(Default::default())));
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
                }
                return Ok(Some(Action::ParticipantCountChanged(self.participant_store.len())));
            }
        };

        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame<'_>, area: Rect) -> Result<()> {
        let theme = Theme::default();
        let [_, area, _] = header_and_two_main_areas(area)?;

        // -=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-
        // Render a border around the entire area
        let block = ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .border_style(theme.border(self.focused))
            .title("Browser controls")
            .title_bottom(Line::from(self.selected.selected_help()).centered());

        frame.render_widget(&block, area);

        let area = block.inner(area);

        // -=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-
        // Layout constraints for the form
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // URL
                Constraint::Length(1), // Fake-media checkbox
                Constraint::Length(1), // Fake-video-file editor
                Constraint::Length(1), // Headless checkbox
                Constraint::Length(2), // Start button
            ])
            .split(area);

        let mut current_row_index = 0;

        // -=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-
        let form_labels = ["URL:", "Fake media:", "Fake video:", "Fake video file:", "Headless:"];
        let max_length = form_labels.iter().map(|s| s.len()).max().unwrap_or(0) + 1;

        // -=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-
        // render individual form widgets for the browser controls

        // --- URL ---
        let url_widget = widgets::label_and_text(
            form_labels[0],
            if self.config.url.is_empty() {
                "<empty>"
            } else {
                &self.config.url
            },
            max_length,
            self.focused && self.selected == SelectedField::Url,
        );
        frame.render_widget(url_widget, rows[current_row_index]);
        current_row_index += 1;

        // --- Fake Media Checkbox ---
        let fake_media_widget = widgets::label_and_bool(
            form_labels[1],
            self.config.fake_media,
            max_length,
            self.focused && self.selected == SelectedField::FakeMedia,
        );
        frame.render_widget(fake_media_widget, rows[current_row_index]);
        current_row_index += 1;

        // --- Fake Video File Input (Conditional) ---
        let vf_widget = widgets::label_and_text(
            form_labels[3],
            self.config.fake_video_file.as_deref().unwrap_or("<empty>"),
            max_length,
            self.focused && self.selected == SelectedField::FakeVideoFile,
        );
        frame.render_widget(vf_widget, rows[current_row_index]);
        current_row_index += 1;

        // --- Headless Checkbox ---
        let headless_widget = widgets::label_and_bool(
            form_labels[4],
            self.config.headless,
            max_length,
            self.focused && self.selected == SelectedField::Headless,
        );
        frame.render_widget(headless_widget, rows[current_row_index]);
        current_row_index += 1;

        // --- Start Button ---
        let start_widget = Paragraph::new("Start Browser")
            .style(if self.focused && self.selected == SelectedField::Start {
                Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow)
            } else {
                Style::default().add_modifier(Modifier::BOLD)
            })
            .block(Block::new().padding(Padding::top(1)));
        frame.render_widget(start_widget, rows[current_row_index]);

        if let Some(editing) = &mut self.editing {
            editing.editor.draw(frame, area)?;
        }

        Ok(())
    }
}
