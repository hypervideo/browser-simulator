use super::Component;
use crate::{
    action::Action,
    config::Config,
    LogCollector,
};
use color_eyre::Result;
use ratatui::{
    layout::Rect,
    style::{
        Color,
        Style,
    },
    widgets::{
        Block,
        List,
        ListItem,
    },
    Frame,
};

#[derive(Debug, Clone, PartialEq)]
pub struct Logs {
    logs: Vec<String>,
    log_collector: LogCollector,
    draw: bool,
}

impl Logs {
    pub fn new(log_collector: LogCollector) -> Self {
        Self {
            logs: log_collector.get_logs(),
            log_collector,
            draw: false,
        }
    }

    fn render_tick(&mut self) -> Result<()> {
        if self.draw {
            self.logs = self.log_collector.get_logs();
        }
        Ok(())
    }
}

impl Component for Logs {
    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        match action {
            Action::Render => self.render_tick()?,
            _ => {}
        }
        Ok(None)
    }

    fn register_config_handler(&mut self, config: Config) -> Result<()> {
        self.draw = config.verbose;
        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        if !self.draw {
            return Ok(());
        }

        let total_logs = self.logs.len();
        let visible_lines = (area.height.saturating_sub(1)) as usize;
        let logs_to_show_count = visible_lines.min(total_logs);

        let log_items: Vec<ListItem> = self
            .logs
            .iter()
            .rev()
            .take(logs_to_show_count)
            .rev()
            .map(|log| ListItem::new(log.as_str()))
            .collect();
        let log_widget = List::new(log_items)
            .block(Block::new().title(format!("Logs ({} total)", total_logs)))
            .highlight_style(Style::default().fg(Color::Cyan));
        frame.render_widget(log_widget, area);
        Ok(())
    }
}
