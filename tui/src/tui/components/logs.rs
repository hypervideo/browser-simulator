use crate::tui::{
    keybindings::{
        KeyBindings,
        Keymap,
    },
    layout::header_and_main_area,
    Action,
    ActivateAction,
    Component,
    FocusedTopLevelComponent,
};
use client_simulator_config::Config;
use color_eyre::Result;
use crossterm::event::KeyCode;
use derive_more::Debug;
use eyre::OptionExt as _;
use ratatui::{
    layout::Rect,
    style::{
        Color,
        Style,
    },
    widgets::Widget as _,
    Frame,
};
use tui_logger::{
    TuiLoggerLevelOutput,
    TuiLoggerSmartWidget,
    TuiWidgetEvent,
    TuiWidgetState,
};

#[derive(Debug)]
pub struct Logs {
    active: bool,
    #[debug(skip)]
    state: TuiWidgetState,
    keymap: Keymap,
}

impl Logs {
    pub fn new() -> Self {
        Self {
            active: false,
            state: TuiWidgetState::new()
                .set_default_display_level(tui_logger::LevelFilter::Debug)
                .set_level_for_target("log", tui_logger::LevelFilter::Info),
            keymap: Keymap::default(),
        }
    }
}

impl Component for Logs {
    fn is_visible(&self) -> bool {
        self.active
    }

    fn is_focused(&self) -> bool {
        self.active
    }

    fn register_config_handler(&mut self, _config: Config, keybindings: KeyBindings) -> Result<()> {
        self.keymap = keybindings
            .get(&FocusedTopLevelComponent::Logs)
            .cloned()
            .ok_or_eyre("No keymap found for Logs")?;
        Ok(())
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        match action {
            Action::Activate(ActivateAction::Logs) => {
                self.active = true;
                return Ok(Some(Action::UpdateGlobalKeybindings(self.keymap.clone())));
            }
            Action::Activate(_) => {
                self.active = false;
            }
            _ => {}
        }
        Ok(None)
    }

    fn handle_key_event(&mut self, key: crossterm::event::KeyEvent) -> Result<Option<Action>> {
        let state = &mut self.state;
        // See https://github.com/gin66/tui-logger?tab=readme-ov-file#smart-widget-key-commands
        match key.code {
            KeyCode::Char(' ') => state.transition(TuiWidgetEvent::SpaceKey),
            KeyCode::Esc => state.transition(TuiWidgetEvent::EscapeKey),
            KeyCode::PageUp => state.transition(TuiWidgetEvent::PrevPageKey),
            KeyCode::PageDown => state.transition(TuiWidgetEvent::NextPageKey),
            KeyCode::Up => state.transition(TuiWidgetEvent::UpKey),
            KeyCode::Down => state.transition(TuiWidgetEvent::DownKey),
            KeyCode::Left => state.transition(TuiWidgetEvent::LeftKey),
            KeyCode::Right => state.transition(TuiWidgetEvent::RightKey),
            KeyCode::Char('+') => state.transition(TuiWidgetEvent::PlusKey),
            KeyCode::Char('-') => state.transition(TuiWidgetEvent::MinusKey),
            KeyCode::Char('h') => state.transition(TuiWidgetEvent::HideKey),
            KeyCode::Char('f') => state.transition(TuiWidgetEvent::FocusKey),
            _ => {}
        };

        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame<'_>, area: Rect) -> Result<()> {
        let [_header_area, area] = header_and_main_area(area)?;

        TuiLoggerSmartWidget::default()
            .style_error(Style::default().fg(Color::Red))
            .style_debug(Style::default().fg(Color::Green))
            .style_warn(Style::default().fg(Color::Yellow))
            .style_trace(Style::default().fg(Color::Magenta))
            .style_info(Style::default().fg(Color::Cyan))
            .output_separator(':')
            .output_timestamp(Some("%H:%M:%S".to_string()))
            .output_level(Some(TuiLoggerLevelOutput::Abbreviated))
            .output_target(true)
            .output_file(true)
            .output_line(true)
            .state(&self.state)
            .render(area, frame.buffer_mut());

        Ok(())
    }
}
