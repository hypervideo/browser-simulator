use super::{
    action::Action,
    components::{
        browser_start::BrowserStart,
        fps::FpsCounter,
        logs::Logs,
        nav_tabs::NavTabs,
        participants::Participants,
        Component,
    },
    tui::{
        Event,
        Tui,
    },
};
use crate::tui::keybindings::{
    KeyBindings,
    Keymap,
};
use client_simulator_browser::participant::{
    ParticipantStore,
    ParticipantWarning,
};
use client_simulator_config::{
    Config,
    TuiArgs,
};
use color_eyre::Result;
use crossterm::event::{
    KeyCode,
    KeyEvent,
    KeyModifiers,
};
use ratatui::{
    layout::{
        Alignment,
        Constraint,
        Direction,
        Layout,
    },
    prelude::Rect,
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
        Wrap,
    },
};
use serde::{
    Deserialize,
    Serialize,
};
use std::collections::HashSet;
use tokio::sync::mpsc;

pub struct App {
    config: Config,
    keybindings: KeyBindings,
    components: Vec<Box<dyn Component>>,
    participants_store: ParticipantStore,
    should_quit: bool,
    should_suspend: bool,
    shutdown_in_progress: bool,
    last_tick_key_events: Vec<KeyEvent>,
    global_keymap: Option<Keymap>,
    warning_modal: Option<WarningModal>,
    seen_warning_keys: HashSet<String>,
}

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FocusedTopLevelComponent {
    #[default]
    BrowserStart,
    Logs,
    Participants,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WarningModal {
    participant: String,
    title: String,
    message: String,
}

type ActionSender = mpsc::UnboundedSender<Action>;
type ActionReceiver = mpsc::UnboundedReceiver<Action>;

impl App {
    pub fn new(args: TuiArgs) -> Result<Self> {
        let config = Config::new(args)?;
        let keybindings = KeyBindings::default();
        let participants_store = ParticipantStore::new(config.data_dir());

        Ok(Self {
            components: vec![
                Box::new(Logs::new()),
                Box::new(Participants::new(participants_store.clone())),
                Box::new(BrowserStart::new(participants_store.clone())),
                Box::new(NavTabs::default()),
                Box::new(FpsCounter::default()),
            ],
            participants_store,
            should_quit: false,
            should_suspend: false,
            shutdown_in_progress: false,
            last_tick_key_events: Vec::new(),
            global_keymap: keybindings.get(&FocusedTopLevelComponent::BrowserStart).cloned(),
            warning_modal: None,
            seen_warning_keys: HashSet::new(),
            config,
            keybindings,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut tui = Tui::new()?
            // .mouse(true) // uncomment this line to enable mouse support
            .tick_rate(1.0)
            .frame_rate(60.0);
        tui.enter()?;

        let (action_tx, mut action_rx) = mpsc::unbounded_channel();

        for component in self.components.iter_mut() {
            component.register_action_handler(action_tx.clone())?;
        }
        for component in self.components.iter_mut() {
            component.register_config_handler(self.config.clone(), self.keybindings.clone())?;
        }
        for component in self.components.iter_mut() {
            component.init(tui.size()?)?;
        }

        let action_tx = action_tx.clone();
        loop {
            self.handle_events(&mut tui, action_tx.clone()).await?;
            self.handle_actions(&mut tui, action_tx.clone(), &mut action_rx)?;
            if self.should_suspend {
                tui.suspend()?;
                action_tx.send(Action::Resume)?;
                action_tx.send(Action::ClearScreen)?;
                // tui.mouse(true);
                tui.enter()?;
            } else if self.should_quit {
                tui.stop()?;
                break;
            }
        }
        tui.exit()?;

        Ok(())
    }

    async fn handle_events(&mut self, tui: &mut Tui, action_tx: ActionSender) -> Result<()> {
        let Some(event) = tui.next_event().await else {
            return Ok(());
        };
        let action_tx = action_tx.clone();
        match event {
            Event::Quit => action_tx.send(Action::Quit)?,
            Event::Tick => action_tx.send(Action::Tick)?,
            Event::Render => action_tx.send(Action::Render)?,
            Event::Resize(x, y) => action_tx.send(Action::Resize(x, y))?,
            Event::Key(key) => self.handle_key_event(key, action_tx.clone())?,
            _ => {}
        }

        for component in self.components.iter_mut() {
            if component.is_focused() {
                if let Some(action) = component.handle_events(Some(event.clone()))? {
                    action_tx.send(action)?;
                }
            }
        }

        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent, action_tx: ActionSender) -> Result<()> {
        if let Some(action) = quit_action_for_key_state(&key, self.shutdown_in_progress) {
            action_tx.send(action)?;
            return Ok(());
        }

        if self.warning_modal.is_some() {
            if matches!(key.code, KeyCode::Enter | KeyCode::Esc) {
                self.warning_modal = None;
            }
            return Ok(());
        }

        let action_tx = action_tx.clone();

        let Some(keymap) = &self.global_keymap else {
            return Ok(());
        };

        match keymap.get(&vec![key]) {
            Some(action) => {
                action_tx.send(action.clone())?;
            }
            _ => {
                // If the key was not handled as a single key action,
                // then consider it for multi-key combinations.
                self.last_tick_key_events.push(key);

                // Check for multi-key combinations
                if let Some(action) = keymap.get(&self.last_tick_key_events) {
                    action_tx.send(action.clone())?;
                }
            }
        }
        Ok(())
    }

    fn handle_actions(&mut self, tui: &mut Tui, action_tx: ActionSender, action_rx: &mut ActionReceiver) -> Result<()> {
        while let Ok(action) = action_rx.try_recv() {
            if action != Action::Tick && action != Action::Render {
                trace!("{action:?}");
            }
            match &action {
                Action::Tick => {}
                Action::Quit => self.begin_shutdown(action_tx.clone()),
                Action::ForceQuit => {
                    if self.shutdown_in_progress {
                        warn!("Force quitting while participant shutdown is still in progress");
                        // Follow-up hardening: force quit currently lets Tokio
                        // tear down tasks that may still own browser driver
                        // handles. A safer force path would track participant
                        // task handles and run a bounded driver cleanup, or
                        // explicitly suppress driver Drop cleanup, before exit.
                        self.should_quit = true;
                    } else {
                        self.begin_shutdown(action_tx.clone());
                    }
                }
                Action::ShutdownComplete => self.should_quit = true,
                Action::Suspend => self.should_suspend = true,
                Action::Resume => self.should_suspend = false,
                Action::ClearScreen => tui.terminal.clear()?,
                Action::Resize(w, h) => self.handle_resize(tui, *w, *h)?,
                Action::Render => self.render(tui)?,
                Action::UpdateGlobalKeybindings(keymap) => {
                    self.global_keymap = Some(keymap.clone());
                }
                _ => {}
            };

            for component in self.components.iter_mut() {
                if let Some(action) = component.update(action.clone())? {
                    action_tx.send(action)?
                };
            }
        }
        Ok(())
    }

    fn begin_shutdown(&mut self, action_tx: ActionSender) {
        if self.shutdown_in_progress {
            return;
        }

        if self.participants_store.is_empty() {
            self.should_quit = true;
            return;
        }

        self.shutdown_in_progress = true;

        let participants_store = self.participants_store.clone();
        tokio::spawn(async move {
            participants_store.shutdown_all().await;
            let _ = action_tx.send(Action::ShutdownComplete);
        });
    }

    fn handle_resize(&mut self, tui: &mut Tui, w: u16, h: u16) -> Result<()> {
        tui.resize(Rect::new(0, 0, w, h))?;
        self.render(tui)?;
        Ok(())
    }

    fn render(&mut self, tui: &mut Tui) -> Result<()> {
        self.poll_warning_modal();
        tui.draw(|frame| {
            // Set uniform background and foreground colors
            frame.render_widget(
                ratatui::widgets::Block::default().style(crate::tui::theme::Theme::default().default),
                frame.area(),
            );

            for component in self.components.iter_mut() {
                if component.is_visible() {
                    if let Err(err) = component.draw(frame, frame.area()) {
                        error!("Failed to draw: {:?}", err);
                    }
                }
            }

            if let Some(modal) = &self.warning_modal {
                render_warning_modal(frame, frame.area(), modal);
            }
        })?;
        Ok(())
    }

    fn poll_warning_modal(&mut self) {
        if self.warning_modal.is_some() {
            return;
        }

        self.warning_modal = next_unseen_warning(self.participants_store.warnings(), &mut self.seen_warning_keys);
    }
}

fn quit_action_for_key_state(key: &KeyEvent, shutdown_in_progress: bool) -> Option<Action> {
    match key.code {
        KeyCode::Char(c) if c.eq_ignore_ascii_case(&'c') && key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(if shutdown_in_progress {
                Action::ForceQuit
            } else {
                Action::Quit
            })
        }
        _ => None,
    }
}

fn next_unseen_warning(
    warnings: Vec<(String, ParticipantWarning)>,
    seen_warning_keys: &mut HashSet<String>,
) -> Option<WarningModal> {
    warnings.into_iter().find_map(|(participant, warning)| {
        let key = warning_key(&participant, &warning);
        if !seen_warning_keys.insert(key) {
            return None;
        }

        Some(WarningModal {
            participant,
            title: warning.title,
            message: warning.message,
        })
    })
}

fn warning_key(participant: &str, warning: &ParticipantWarning) -> String {
    format!("{participant}\n{}\n{}", warning.title, warning.message)
}

fn render_warning_modal(frame: &mut ratatui::Frame<'_>, area: Rect, modal: &WarningModal) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let width = popup_axis_length(area.width, 4, 24, 82);
    let height = popup_axis_length(area.height, 2, 7, 10);
    let popup = centered_rect(width, height, area);
    let block = Block::default()
        .title("Warning")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .style(Style::default().fg(Color::White).bg(Color::Black));
    let text = vec![
        Line::from(Span::styled(
            modal.title.clone(),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!("Participant: {}", modal.participant),
            Style::default().fg(Color::Gray),
        )),
        Line::from(""),
        Line::from(modal.message.clone()),
        Line::from(""),
        Line::from(Span::styled("Enter/Esc to dismiss", Style::default().fg(Color::Gray))),
    ];

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(text)
            .block(block)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true }),
        popup,
    );
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(area.height.saturating_sub(height) / 2),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(area.width.saturating_sub(width) / 2),
            Constraint::Length(width),
            Constraint::Min(0),
        ])
        .split(vertical[1]);

    horizontal[1]
}

fn popup_axis_length(available: u16, padding: u16, min: u16, max: u16) -> u16 {
    let padded = available.saturating_sub(padding).min(max);
    padded.max(min.min(available))
}

#[cfg(test)]
mod tests {
    use super::{
        next_unseen_warning,
        quit_action_for_key_state,
    };
    use crate::tui::action::Action;
    use client_simulator_browser::participant::ParticipantWarning;
    use crossterm::event::{
        KeyCode,
        KeyEvent,
        KeyModifiers,
    };
    use std::collections::HashSet;

    #[test]
    fn first_ctrl_c_requests_graceful_shutdown() {
        let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);

        assert_eq!(quit_action_for_key_state(&key, false), Some(Action::Quit));
    }

    #[test]
    fn second_ctrl_c_forces_quit_while_shutdown_is_running() {
        let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);

        assert_eq!(quit_action_for_key_state(&key, true), Some(Action::ForceQuit));
    }

    #[test]
    fn next_unseen_warning_returns_each_warning_once() {
        let warning = ParticipantWarning::new("AWS Device Farm credentials", "Run setup-auth");
        let mut seen = HashSet::new();

        let modal = next_unseen_warning(vec![("sim-user".to_string(), warning.clone())], &mut seen)
            .expect("first warning should be shown");

        assert_eq!(modal.participant, "sim-user");
        assert_eq!(modal.title, "AWS Device Farm credentials");
        assert!(next_unseen_warning(vec![("sim-user".to_string(), warning)], &mut seen).is_none());
    }
}
