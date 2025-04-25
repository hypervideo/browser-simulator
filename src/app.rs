use crate::{
    browser::WebBrowser,
    config::Config,
};
use color_eyre::Result;
use crossterm::event::{
    self,
    Event,
};
use ratatui::{
    crossterm::event::KeyCode,
    layout::{
        Constraint,
        Direction,
        Layout,
        Rect,
    },
    style::{
        Color,
        Modifier,
        Style,
        Stylize as _,
    },
    widgets::{
        Block,
        Clear,
        Paragraph,
    },
    Frame,
};
use std::time::Duration;

#[derive(Debug)]
pub(crate) struct Model {
    config: Config,
    selected: SelectedField,
    editing: Option<EditingState>,
    pub(crate) running_state: RunningState,
}

impl Model {
    // Accept the prepared config object
    pub(crate) fn new(config: Config) -> Self {
        Self {
            config,
            selected: Default::default(),
            editing: None,
            running_state: Default::default(),
        }
    }
}

#[derive(Debug)]
struct EditingState {
    field: SelectedField,
    buffer: String,
}

impl EditingState {
    fn title(&self) -> &'static str {
        match self.field {
            SelectedField::Url => "Edit URL",
            SelectedField::Cookie => "Edit Cookie",
            SelectedField::FakeVideoFile => "Edit Fake Video File", // new
            SelectedField::Start | SelectedField::FakeMedia | SelectedField::FakeVideo => "",
        }
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
enum SelectedField {
    #[default]
    Url,
    Cookie,
    FakeMedia,
    FakeVideo,
    FakeVideoFile,
    Start,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) enum RunningState {
    #[default]
    Running,
    Done,
}

#[derive(PartialEq)]
pub(crate) enum Message {
    Quit,
    MoveUp,
    MoveDown,
    StartEdit,
    CancelEdit,
    ConfirmEdit,
    EditChar(char),
    EditBackspace,
    DeleteCookie,
    StartBrowser,
    ToggleFakeMedia,
    ToggleFakeVideo,
    DeleteFakeVideoFile,
}

pub(crate) fn view(model: &mut Model, frame: &mut Frame) {
    // Dynamically create constraints based on UI elements
    let mut constraints = vec![
        Constraint::Length(3), // URL
        Constraint::Length(3), // Cookie
        Constraint::Length(3), // Fake-media checkbox
        Constraint::Length(3), // Fake-video checkbox
    ];
    if model.config.fake_video_file.is_some() {
        constraints.push(Constraint::Length(3)); // Fake-video-file editor
    }
    constraints.push(Constraint::Length(1)); // Spacer before Start
    constraints.push(Constraint::Length(3)); // Start button

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints) // Use the dynamic constraints
        .split(frame.area());

    let mut current_row_index = 0;

    // --- URL ---
    let url = model.config.url.clone();
    let url_widget =
        Paragraph::new(url)
            .block(Block::bordered().title("URL"))
            .style(if model.selected == SelectedField::Url {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            });
    frame.render_widget(url_widget, rows[current_row_index]);
    current_row_index += 1;

    // Cookie
    let mut cookie = model.config.cookie.clone();
    if cookie.is_empty() {
        cookie = "<empty>".to_string();
    } else if cookie.len() > 30 {
        cookie.truncate(30);
        cookie.push_str("...");
    }
    let cookie_widget = Paragraph::new(cookie)
        .block(Block::bordered().title("Cookie (x to clear)"))
        .style(if model.selected == SelectedField::Cookie {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        });
    frame.render_widget(cookie_widget, rows[current_row_index]);
    current_row_index += 1;

    // --- Fake Media Checkbox ---
    let fake_media_txt = format!("{} Use fake media", if model.config.fake_media { "[x]" } else { "[ ]" });
    let fake_media_widget =
        Paragraph::new(fake_media_txt)
            .block(Block::bordered())
            .style(if model.selected == SelectedField::FakeMedia {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            });
    frame.render_widget(fake_media_widget, rows[current_row_index]);
    current_row_index += 1;

    // --- Fake Video Checkbox ---
    let fake_video_txt = format!(
        "{} Enable fake video source",
        if model.config.fake_video_file.is_some() {
            "[x]"
        } else {
            "[ ]"
        }
    );
    let fake_video_widget =
        Paragraph::new(fake_video_txt)
            .block(Block::bordered())
            .style(if model.selected == SelectedField::FakeVideo {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            });
    frame.render_widget(fake_video_widget, rows[current_row_index]);
    current_row_index += 1;

    // --- Fake Video File Input (Conditional) ---
    if let Some(path) = &model.config.fake_video_file {
        let display = if path.is_empty() { "<empty>" } else { path };
        let vf_widget = Paragraph::new(display)
            .block(Block::bordered().title("Fake video file (x to clear)"))
            .style(if model.selected == SelectedField::FakeVideoFile {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            });
        frame.render_widget(vf_widget, rows[current_row_index]);
        current_row_index += 1;
    }

    // Skip the spacer row index
    current_row_index += 1;

    // --- Start Button ---
    let start_widget = Paragraph::new("Start Browser")
        .block(Block::bordered().border_style(Style::new().white()))
        .style(if model.selected == SelectedField::Start {
            Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow)
        } else {
            Style::default().add_modifier(Modifier::BOLD)
        });
    frame.render_widget(start_widget, rows[current_row_index]);

    // popup
    if let Some(edit) = &model.editing {
        let area = centered_rect(60, 20, frame.area());
        frame.render_widget(Clear, area); // nettoie la zone
        let popup = Paragraph::new(edit.buffer.as_str()).block(Block::bordered().title(edit.title()));
        frame.render_widget(popup, area);
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

/// Convert Event to Message
///
/// We don't need to pass in a `model` to this function in this example
/// but you might need it as your project evolves
pub(crate) fn handle_event(model: &Model) -> Result<Option<Message>> {
    if event::poll(Duration::from_millis(250))? {
        if let Event::Key(key) = event::read()? {
            if key.kind == event::KeyEventKind::Press {
                return Ok(handle_key(model, key));
            }
        }
    }
    Ok(None)
}

fn handle_key(model: &Model, key: event::KeyEvent) -> Option<Message> {
    let editing = model.editing.is_some();
    match key.code {
        // global
        KeyCode::Char('q') if !editing => Some(Message::Quit),
        KeyCode::Char('x')
            if !editing && model.selected == SelectedField::Cookie && !model.config.cookie.is_empty() =>
        {
            Some(Message::DeleteCookie)
        }

        // navigation
        KeyCode::Up if !editing => Some(Message::MoveUp),
        KeyCode::Down if !editing => Some(Message::MoveDown),

        // start editing or start browser or toggle
        KeyCode::Enter if !editing && model.selected == SelectedField::Start => Some(Message::StartBrowser),
        KeyCode::Enter if !editing && model.selected == SelectedField::FakeMedia => Some(Message::ToggleFakeMedia),
        KeyCode::Enter if !editing && model.selected == SelectedField::FakeVideo => Some(Message::ToggleFakeVideo),
        KeyCode::Enter
            if !editing
                && matches!(
                    model.selected,
                    SelectedField::Url | SelectedField::Cookie | SelectedField::FakeVideoFile
                ) =>
        {
            Some(Message::StartEdit)
        } // Edit URL/Cookie/FakeVideoFile

        // delete fake video file
        KeyCode::Char('x')
            if !editing && model.selected == SelectedField::FakeVideoFile && model.config.fake_video_file.is_some() =>
        {
            Some(Message::DeleteFakeVideoFile)
        }

        // Popup
        KeyCode::Esc if editing => Some(Message::CancelEdit),
        KeyCode::Enter if editing => Some(Message::ConfirmEdit),
        KeyCode::Backspace if editing => Some(Message::EditBackspace),
        KeyCode::Char(c) if editing => Some(Message::EditChar(c)),

        _ => None,
    }
}

pub(crate) fn update(model: &mut Model, msg: Message) -> Option<Message> {
    match msg {
        Message::Quit => {
            model.running_state = RunningState::Done;
        }

        Message::MoveUp => {
            model.selected = match model.selected {
                SelectedField::Cookie => SelectedField::Url,
                SelectedField::FakeMedia => SelectedField::Cookie,
                SelectedField::FakeVideo => SelectedField::FakeMedia,
                SelectedField::FakeVideoFile => SelectedField::FakeVideo,
                SelectedField::Start => {
                    if model.config.fake_video_file.is_some() {
                        SelectedField::FakeVideoFile // Go to file input if visible
                    } else {
                        SelectedField::FakeVideo // Otherwise go to video checkbox
                    }
                }
                other => other, // Url stays Url
            };
        }

        Message::MoveDown => {
            model.selected = match model.selected {
                SelectedField::Url => SelectedField::Cookie,
                SelectedField::Cookie => SelectedField::FakeMedia,
                SelectedField::FakeMedia => SelectedField::FakeVideo,
                SelectedField::FakeVideo => {
                    if model.config.fake_video_file.is_some() {
                        SelectedField::FakeVideoFile // Go to file input if visible
                    } else {
                        SelectedField::Start // Otherwise skip to Start
                    }
                }
                SelectedField::FakeVideoFile => SelectedField::Start,
                SelectedField::Start => SelectedField::Url, /* Loop back to top */
            };
        }

        // Edit
        Message::StartEdit => {
            if model.editing.is_none() {
                let buffer = match model.selected {
                    SelectedField::Url => model.config.url.clone(),
                    SelectedField::Cookie => model.config.cookie.clone(),
                    SelectedField::FakeVideoFile => model.config.fake_video_file.clone().unwrap_or_default(),
                    _ => String::new(), // Should not happen for Start/FakeMedia/FakeVideo
                };
                model.editing = Some(EditingState {
                    field: model.selected,
                    buffer,
                });
            }
        }

        Message::CancelEdit => {
            model.editing = None;
        }

        Message::ConfirmEdit => {
            if let Some(edit) = model.editing.take() {
                match edit.field {
                    SelectedField::Url => model.config.url = edit.buffer,
                    SelectedField::Cookie => model.config.cookie = edit.buffer,
                    SelectedField::FakeVideoFile => {
                        // Set to None if the buffer is empty or only whitespace
                        model.config.fake_video_file = if edit.buffer.trim().is_empty() {
                            None
                        } else {
                            Some(edit.buffer)
                        };
                    }
                    _ => {} // Should not happen for Start/FakeMedia/FakeVideo
                }
                // Save config immediately after edit confirmation
                if let Err(e) = model.config.save() {
                    error!(?e, "Failed to save config after edit");
                    // Optionally, inform the user via TUI state
                }
            }
        }

        Message::EditChar(c) => {
            if let Some(edit) = &mut model.editing {
                edit.buffer.push(c);
            }
        }

        Message::DeleteCookie => {
            model.config.cookie.clear(); // supprime le cookie
            if let Err(e) = model.config.save() {
                error!(?e, "Failed to save config after deleting cookie");
                // TODO: inform the user via TUI state
            }
        }

        Message::EditBackspace => {
            if let Some(edit) = &mut model.editing {
                edit.buffer.pop();
            }
        }

        Message::ToggleFakeMedia => {
            model.config.fake_media = !model.config.fake_media;
            if let Err(e) = model.config.save() {
                error!(?e, "Failed to save config after toggling fake media");
            }
        }
        Message::ToggleFakeVideo => {
            if model.config.fake_video_file.is_some() {
                model.config.fake_video_file = None;
                // If toggling off, ensure selection moves if it was on FakeVideoFile
                if model.selected == SelectedField::FakeVideoFile {
                    model.selected = SelectedField::FakeVideo;
                }
            } else {
                // Default to empty string, user needs to edit it
                model.config.fake_video_file = Some(String::new());
            }
            if let Err(e) = model.config.save() {
                error!(?e, "Failed to save config after toggling fake video");
            }
        }
        Message::DeleteFakeVideoFile => {
            model.config.fake_video_file = None;
            // Ensure selection moves if it was on FakeVideoFile
            if model.selected == SelectedField::FakeVideoFile {
                model.selected = SelectedField::FakeVideo;
            }
            if let Err(e) = model.config.save() {
                error!(?e, "Failed to save config after deleting fake video file");
            }
        }

        // Start Browser / Playwright
        Message::StartBrowser => {
            if model.editing.is_some() {
                return None;
            }
            let config = model.config.clone();
            info!(
                "Starting browser with URL: {}, Cookie: ..., Use fake media: {}, Fake video: {}",
                config.url,
                config.fake_media,
                config.fake_video_file.as_deref().unwrap_or("<none>")
            );
            tokio::spawn(async move {
                if let Err(err) = WebBrowser::hyper_hyper(
                    config.cookie,
                    config.url,
                    config.fake_media,
                    config.fake_video_file.clone(),
                )
                .await
                {
                    error!("Failed to start browser: {:?}", err);
                }
            });
        }
    };
    None
}
