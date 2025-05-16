use crate::{
    action::Action,
    browser::participant::ParticipantStore,
    components::{
        browser_start::BrowserStart,
        fps::FpsCounter,
        logs::Logs,
        modal::{
            TextInputModal,
            TextModalAction,
        },
        participants::Participants,
        Component,
    },
    config::Config,
    tui::{
        Event,
        Tui,
    },
    LogCollector,
};
use color_eyre::Result;
use crossterm::event::KeyEvent;
use ratatui::{
    layout::{
        Constraint,
        Direction,
        Layout,
        Size,
    },
    prelude::Rect,
    style::{
        Color,
        Modifier,
        Style,
    },
    widgets::{
        Block,
        Borders,
        Tabs,
    },
};
use serde::{
    Deserialize,
    Serialize,
};
use tokio::sync::mpsc;

pub struct App {
    config: Config,
    should_quit: bool,
    should_suspend: bool,
    last_tick_key_events: Vec<KeyEvent>,
    mode: Mode,
    logs: Logs,
    browser_start: BrowserStart,
    participants: Participants,
    modal: Option<TextInputModal>,
    fps_counter: FpsCounter,
}

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    BrowserStart,
    Logs,
    Participants,
}

type ActionSender = mpsc::UnboundedSender<Action>;
type ActionReceiver = mpsc::UnboundedReceiver<Action>;

impl App {
    pub fn new(args: crate::Args, log_collector: LogCollector) -> Result<Self> {
        let participants_store = ParticipantStore::new();

        Ok(Self {
            should_quit: false,
            should_suspend: false,
            config: Config::new(args)?,
            last_tick_key_events: Vec::new(),
            mode: Mode::BrowserStart,
            logs: Logs::new(log_collector),
            browser_start: BrowserStart::new(participants_store.clone()),
            participants: Participants::new(participants_store.clone()),
            fps_counter: FpsCounter::new(),
            modal: None,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut tui = Tui::new()?
            // .mouse(true) // uncomment this line to enable mouse support
            .tick_rate(1.0)
            .frame_rate(60.0);
        tui.enter()?;

        let (action_tx, mut action_rx) = mpsc::unbounded_channel();

        self.register_action_handler(action_tx.clone()).await?;
        self.register_config_handler(self.config.clone()).await?;
        self.init(tui.size()?).await?;

        let action_tx = action_tx.clone();
        loop {
            self.handle_events(&mut tui, action_tx.clone()).await?;
            self.handle_actions(&mut tui, action_tx.clone(), &mut action_rx).await?;
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
        self.handle_component_events(event, action_tx).await?;

        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent, action_tx: ActionSender) -> Result<()> {
        if self.modal.is_some() {
            return Ok(());
        }

        let action_tx = action_tx.clone();
        let Some(keymap) = self.config.keybindings.get(&self.mode) else {
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

    async fn handle_actions(
        &mut self,
        tui: &mut Tui,
        action_tx: ActionSender,
        action_rx: &mut ActionReceiver,
    ) -> Result<()> {
        let ignore_actions = vec![
            Action::Tick,
            Action::Render,
            Action::Logs,
            Action::BrowserStart,
            Action::Participants,
        ];

        while let Ok(mut action) = action_rx.try_recv() {
            if !ignore_actions.contains(&action) {
                debug!("Got action: {action:?}");
            }

            match &action {
                Action::Tick => {}
                Action::Quit => self.should_quit = true,
                Action::Suspend => self.should_suspend = true,
                Action::Resume => self.should_suspend = false,
                Action::BrowserStart => {
                    self.show_browser_start()?;
                }
                Action::Participants => {
                    self.show_participants()?;
                }
                Action::Logs => {
                    self.show_logs()?;
                }
                Action::ClearScreen => tui.terminal.clear()?,
                Action::Resize(w, h) => self.handle_resize(tui, *w, *h)?,
                Action::Render => self.render(tui)?,
                Action::TextModal(TextModalAction::ShowTextModal { title, content }) => {
                    self.modal = Some(TextInputModal::new(title, content));
                }
                Action::TextModal(TextModalAction::TextModalSubmit(content)) => {
                    self.modal = None;
                    self.mode = Mode::BrowserStart;
                    action = Action::TextModal(TextModalAction::TextModalSubmit(content.to_string()));
                }
                Action::TextModal(TextModalAction::TextModalCancel) => {
                    self.modal = None;
                    self.mode = Mode::BrowserStart;
                }
                _ => {}
            };

            self.update(action, action_tx.clone()).await?;
        }
        Ok(())
    }

    fn handle_resize(&mut self, tui: &mut Tui, w: u16, h: u16) -> Result<()> {
        tui.resize(Rect::new(0, 0, w, h))?;
        self.render(tui)?;
        Ok(())
    }

    fn render(&mut self, tui: &mut Tui) -> Result<()> {
        tui.draw(|frame| {
            if let Some(ref mut modal) = self.modal {
                if let Err(e) = modal.draw(frame, frame.area()) {
                    error!("Error rendering modal: {e}");
                }

                return;
            }

            // Split the screen: main content and logs
            let constraints = vec![
                Constraint::Max(3), // Header
                Constraint::Min(0), // Main area
            ];

            let [header_area, main_area] = *Layout::default()
                .direction(Direction::Vertical)
                .constraints(constraints)
                .split(frame.area())
            else {
                return;
            };

            // Define tab titles for each Mode
            let tab_titles = vec![
                "[1] Browser".to_string(),
                format!("[2] Participants ({})", self.participants.len()),
                format!("[3] Logs ({})", self.logs.count()),
            ];
            let selected_tab = match self.mode {
                Mode::BrowserStart => 0,
                Mode::Participants => 1,
                Mode::Logs => 2,
            };

            // Create the Tabs widget
            let tabs = Tabs::new(tab_titles)
                .block(Block::default().borders(Borders::ALL).title("Modes"))
                .select(selected_tab)
                .style(Style::default().bg(Color::DarkGray).fg(Color::White))
                .highlight_style(Style::default().add_modifier(Modifier::BOLD))
                .divider("|");

            // Render the tabs in the header area
            frame.render_widget(tabs, header_area);
            if let Err(e) = self.fps_counter.draw(frame, header_area) {
                error!("Error rendering FPS counter: {e}");
            }

            // Render the main content based on the current mode
            match self.mode {
                Mode::BrowserStart => {
                    if let Err(e) = self.browser_start.draw(frame, main_area) {
                        error!("Error rendering browser start: {e}");
                    }
                }
                Mode::Participants => {
                    if let Err(e) = self.participants.draw(frame, main_area) {
                        error!("Error rendering participants: {e}");
                    }
                }
                Mode::Logs => {
                    if let Err(e) = self.logs.draw(frame, main_area) {
                        error!("Error rendering logs: {e}");
                    }
                }
            }
        })?;
        Ok(())
    }

    fn show_logs(&mut self) -> Result<()> {
        self.browser_start.suspend()?;
        self.participants.suspend()?;

        self.logs.resume()?;
        self.mode = Mode::Logs;
        Ok(())
    }
    fn show_browser_start(&mut self) -> Result<()> {
        self.participants.suspend()?;
        self.logs.suspend()?;

        self.browser_start.resume()?;
        self.mode = Mode::BrowserStart;
        Ok(())
    }
    fn show_participants(&mut self) -> Result<()> {
        self.browser_start.suspend()?;
        self.logs.suspend()?;

        self.participants.resume()?;
        self.mode = Mode::Participants;
        Ok(())
    }

    async fn register_action_handler(&mut self, action_tx: ActionSender) -> Result<()> {
        self.logs.register_action_handler(action_tx.clone())?;
        self.browser_start.register_action_handler(action_tx.clone())?;
        self.fps_counter.register_action_handler(action_tx.clone())?;
        Ok(())
    }

    async fn register_config_handler(&mut self, config: Config) -> Result<()> {
        self.logs.register_config_handler(config.clone())?;
        self.browser_start.register_config_handler(config.clone())?;
        self.fps_counter.register_config_handler(config.clone())?;
        Ok(())
    }

    async fn init(&mut self, size: Size) -> Result<()> {
        self.logs.init(size)?;
        self.browser_start.init(size)?;
        self.fps_counter.init(size)?;
        Ok(())
    }

    async fn handle_component_events(&mut self, event: Event, action_tx: ActionSender) -> Result<()> {
        if let Some(ref mut modal) = self.modal {
            if let Some(action) = modal.handle_events(Some(event))? {
                action_tx.send(action)?;
            }
            return Ok(());
        }

        match self.mode {
            Mode::BrowserStart => {
                if let Some(action) = self.browser_start.handle_events(Some(event))? {
                    action_tx.send(action)?;
                }
            }
            Mode::Logs => {
                if let Some(action) = self.logs.handle_events(Some(event))? {
                    action_tx.send(action)?;
                }
            }
            Mode::Participants => {
                if let Some(action) = self.participants.handle_events(Some(event))? {
                    action_tx.send(action)?;
                }
            }
        };

        Ok(())
    }

    async fn update(&mut self, action: Action, action_tx: ActionSender) -> Result<()> {
        if let Some(ref mut modal) = self.modal {
            if let Some(action) = modal.update(action)? {
                action_tx.send(action)?;
            }
            return Ok(());
        }

        match self.mode {
            Mode::BrowserStart => {
                if let Some(action) = self.browser_start.update(action)? {
                    action_tx.send(action)?;
                }
            }
            Mode::Logs => {
                if let Some(action) = self.logs.update(action)? {
                    action_tx.send(action)?;
                }
            }
            Mode::Participants => {
                if let Some(action) = self.participants.update(action)? {
                    action_tx.send(action)?;
                }
            }
        };
        Ok(())
    }
}
