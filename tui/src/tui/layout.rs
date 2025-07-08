use eyre::{
    bail,
    Result,
};
use ratatui::{
    layout::{
        Constraint,
        Direction,
        Flex,
        Layout,
    },
    prelude::Rect,
};

/// Split the screen: main content and nav header
pub(crate) fn header_and_main_area(area: Rect) -> Result<[Rect; 2]> {
    let constraints = vec![
        Constraint::Max(2), // Header
        Constraint::Min(0), // Main area
    ];

    let [header_area, area] = *Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area)
    else {
        bail!("Failed to split the area");
    };

    Ok([header_area, area])
}

/// Split the screen: main content and nav header
pub(crate) fn header_and_two_main_areas(area: Rect) -> Result<[Rect; 3]> {
    let [header, area] = header_and_main_area(area)?;
    let [a, b] = *Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Max(16), Constraint::Min(0)])
        .split(area)
    else {
        bail!("Failed to split the area");
    };
    Ok([header, a, b])
}

/// Centers a [`Rect`] within another [`Rect`] using the provided [`Constraint`]s.
///
/// # Examples
///
/// ```rust
/// use ratatui::layout::{Constraint, Rect};
///
/// let area = Rect::new(0, 0, 100, 100);
/// let horizontal = Constraint::Percentage(20);
/// let vertical = Constraint::Percentage(30);
///
/// let centered = center(area, horizontal, vertical);
/// ```
pub(crate) fn center(area: Rect, horizontal: Constraint, vertical: Constraint) -> Rect {
    let [area] = Layout::horizontal([horizontal]).flex(Flex::Center).areas(area);
    let [area] = Layout::vertical([vertical]).flex(Flex::Center).areas(area);
    area
}
