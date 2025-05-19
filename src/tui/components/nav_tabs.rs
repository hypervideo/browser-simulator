use crate::tui::{
    layout,
    Action,
    ActivateAction,
    Component,
};
use eyre::{
    bail,
    Result,
};
use ratatui::{
    layout::{
        Constraint,
        Direction,
        Layout,
    },
    style::{
        Color,
        Modifier,
        Style,
    },
    widgets::Tabs,
};

#[derive(Debug, Default)]
pub struct NavTabs {
    screen: Screen,
    participant_count: usize,
}

#[derive(Debug, Default)]
enum Screen {
    #[default]
    BrowserStart,
    Logs,
}

impl NavTabs {}

impl Component for NavTabs {
    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        match action {
            Action::Activate(ActivateAction::BrowserStart) | Action::Activate(ActivateAction::Participants) => {
                self.screen = Screen::BrowserStart;
            }
            Action::Activate(ActivateAction::Logs) => {
                self.screen = Screen::Logs;
            }
            Action::ParticipantCountChanged(participant_count) => {
                self.participant_count = participant_count;
            }
            _ => {}
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut ratatui::Frame, area: ratatui::prelude::Rect) -> Result<()> {
        let [header_area, _main_area] = layout::header_and_main_area(area)?;
        let [header_area] = *Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Max(2)])
            .split(header_area)
        else {
            bail!("Failed to split the area");
        };

        // Define tab titles for each Mode
        let tab_titles = vec!["Browser [1]".to_string(), format!("Logs [2]")];

        let selected_tab = match self.screen {
            Screen::BrowserStart => 0,
            Screen::Logs => 1,
        };

        // Create the Tabs widget
        let tabs = Tabs::new(tab_titles)
            .select(selected_tab)
            .style(Style::default().fg(Color::Gray))
            .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            .block(ratatui::widgets::Block::new().borders(ratatui::widgets::Borders::BOTTOM))
            .divider(" | ");

        // Render the tabs in the header area
        frame.render_widget(tabs, header_area);

        Ok(())
    }
}
