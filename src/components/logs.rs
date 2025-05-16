use super::Component;
use crate::{
    action::Action,
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

    pub fn count(&self) -> usize {
        self.log_collector.get_logs().len()
    }
}

impl Component for Logs {
    fn suspend(&mut self) -> Result<()> {
        self.draw = false;
        Ok(())
    }
    fn resume(&mut self) -> Result<()> {
        self.draw = true;
        Ok(())
    }
    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        #[allow(clippy::single_match)]
        match action {
            Action::Render => self.render_tick()?,
            _ => {}
        }
        Ok(None)
    }
    fn draw(&mut self, frame: &mut Frame<'_>, area: Rect) -> Result<()> {
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
        let log_widget = List::new(log_items).highlight_style(Style::default().fg(Color::Cyan));
        frame.render_widget(log_widget, area);
        Ok(())
    }
}
