use crate::{
    action::Action,
    components::{
        browser_start::BrowserStart,
        fps::FpsCounter,
        logs::Logs,
        modal::{
            TextInputModal,
            TextModalAction,
        },
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
    },
    prelude::Rect,
};
use serde::{
    Deserialize,
    Serialize,
};
use tokio::sync::mpsc;

pub struct App {
    config: Config,
    components: Vec<Box<dyn Component>>,
    logs: Box<dyn Component>,
    should_quit: bool,
    should_suspend: bool,
    last_tick_key_events: Vec<KeyEvent>,
    mode: Mode,
}

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    BrowserStart,
    TextInputModal,
}

type ActionSender = mpsc::UnboundedSender<Action>;
type ActionReceiver = mpsc::UnboundedReceiver<Action>;

impl App {
    pub fn new(args: crate::Args, log_collector: LogCollector) -> Result<Self> {
        Ok(Self {
            components: vec![Box::new(BrowserStart::new()), Box::new(FpsCounter::default())],
            logs: Box::new(Logs::new(log_collector)),
            should_quit: false,
            should_suspend: false,
            config: Config::new(args)?,
            last_tick_key_events: Vec::new(),
            mode: Mode::BrowserStart,
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
            component.register_config_handler(self.config.clone())?;
        }
        for component in self.components.iter_mut() {
            component.init(tui.size()?)?;
        }

        self.logs.register_action_handler(action_tx.clone())?;
        self.logs.register_config_handler(self.config.clone())?;
        self.logs.init(tui.size()?)?;

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
            if let Some(action) = component.handle_events(Some(event.clone()))? {
                action_tx.send(action)?;
            }
        }

        if let Some(action) = self.logs.handle_events(Some(event))? {
            action_tx.send(action)?;
        }

        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent, action_tx: ActionSender) -> Result<()> {
        let action_tx = action_tx.clone();
        let Some(keymap) = self.config.keybindings.get(&self.mode) else {
            return Ok(());
        };
        match keymap.get(&vec![key]) {
            Some(action) => {
                info!("Got action: {action:?}");
                action_tx.send(action.clone())?;
            }
            _ => {
                // If the key was not handled as a single key action,
                // then consider it for multi-key combinations.
                self.last_tick_key_events.push(key);

                // Check for multi-key combinations
                if let Some(action) = keymap.get(&self.last_tick_key_events) {
                    info!("Got action: {action:?}");
                    action_tx.send(action.clone())?;
                }
            }
        }
        Ok(())
    }

    fn handle_actions(&mut self, tui: &mut Tui, action_tx: ActionSender, action_rx: &mut ActionReceiver) -> Result<()> {
        while let Ok(mut action) = action_rx.try_recv() {
            if action != Action::Tick && action != Action::Render {
                debug!("{action:?}");
            }
            match &action {
                Action::Tick => {}
                Action::Quit => self.should_quit = true,
                Action::Suspend => self.should_suspend = true,
                Action::Resume => self.should_suspend = false,
                Action::ClearScreen => tui.terminal.clear()?,
                Action::Resize(w, h) => self.handle_resize(tui, *w, *h)?,
                Action::Render => self.render(tui)?,
                Action::TextModal(TextModalAction::ShowTextModal { title, content }) => {
                    let modal = TextInputModal::new(title, content);
                    self.components.push(Box::new(modal));
                    self.mode = Mode::TextInputModal;
                }
                Action::TextModal(TextModalAction::TextModalSubmit(content)) => {
                    self.components.retain(|component| !component.is_modal());
                    self.mode = Mode::BrowserStart;
                    action = Action::TextModal(TextModalAction::TextModalSubmit(content.to_string()));
                }
                Action::TextModal(TextModalAction::TextModalCancel) => {
                    self.components.retain(|component| !component.is_modal());
                    self.mode = Mode::BrowserStart;
                }
                _ => {}
            };
            for component in self.components.iter_mut() {
                if let Some(action) = component.update(action.clone())? {
                    action_tx.send(action)?
                };
            }
            if let Some(action) = self.logs.update(action.clone())? {
                action_tx.send(action)?
            };
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
            let is_modal = self.mode == Mode::TextInputModal;

            if is_modal {
                // Draw the modal first
                let modal = self.components.last_mut().unwrap();
                if let Err(err) = modal.draw(frame, frame.area()) {
                    error!("Failed to draw: {:?}", err);
                }
                return;
            }

            // Split the screen: main content and logs
            let constraints = vec![
                Constraint::Min(20), // Main content (BrowserStart, FpsCounter)
                Constraint::Min(0),  // Logs
            ];
            let areas = Layout::default()
                .direction(Direction::Vertical)
                .constraints(constraints)
                .split(frame.area());

            let main_area = areas[0];
            let log_area = areas[1];

            for component in self.components.iter_mut() {
                if let Err(err) = component.draw(frame, main_area) {
                    error!("Failed to draw: {:?}", err);
                }
            }

            if let Err(err) = self.logs.draw(frame, log_area) {
                error!("Failed to draw logs: {:?}", err);
            }
        })?;
        Ok(())
    }
}
