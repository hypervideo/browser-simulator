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
use client_simulator_browser::participant::ParticipantStore;
use client_simulator_config::{
    Args,
    Config,
};
use color_eyre::Result;
use crossterm::event::KeyEvent;
use ratatui::prelude::Rect;
use serde::{
    Deserialize,
    Serialize,
};
use tokio::sync::mpsc;

pub struct App {
    config: Config,
    keybindings: KeyBindings,
    components: Vec<Box<dyn Component>>,
    should_quit: bool,
    should_suspend: bool,
    last_tick_key_events: Vec<KeyEvent>,
    global_keymap: Option<Keymap>,
}

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FocusedTopLevelComponent {
    #[default]
    BrowserStart,
    Logs,
    Participants,
}

type ActionSender = mpsc::UnboundedSender<Action>;
type ActionReceiver = mpsc::UnboundedReceiver<Action>;

impl App {
    pub fn new(args: Args) -> Result<Self> {
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
            should_quit: false,
            should_suspend: false,
            last_tick_key_events: Vec::new(),
            global_keymap: keybindings.get(&FocusedTopLevelComponent::BrowserStart).cloned(),
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
                Action::Quit => self.should_quit = true,
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

    fn handle_resize(&mut self, tui: &mut Tui, w: u16, h: u16) -> Result<()> {
        tui.resize(Rect::new(0, 0, w, h))?;
        self.render(tui)?;
        Ok(())
    }

    fn render(&mut self, tui: &mut Tui) -> Result<()> {
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
        })?;
        Ok(())
    }
}
