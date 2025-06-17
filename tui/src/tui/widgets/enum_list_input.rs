use super::ListInput;
use eyre::{
    bail,
    eyre,
    Result,
};
use std::str::FromStr;

#[derive(Debug)]
pub(crate) struct EnumListInput<T> {
    inner: ListInput<String>,
    __phantom: std::marker::PhantomData<T>,
}

impl<T> EnumListInput<T>
where
    T: ToString + PartialEq + FromStr,
{
    pub(crate) fn new(title: &'static str, items: impl IntoIterator<Item = impl Into<T>>, selected: T) -> Self {
        let values = items.into_iter().map(|item| item.into()).collect::<Vec<T>>();
        let selected = values.iter().position(|model| model == &selected);
        let items = values.iter().map(|model| (model.to_string(), model.to_string()));
        Self {
            inner: ListInput::<String>::new(title, items, selected),
            __phantom: std::marker::PhantomData,
        }
    }

    pub(crate) fn draw(&mut self, frame: &mut ratatui::Frame<'_>, _area: ratatui::prelude::Rect) -> Result<()> {
        self.inner.draw(frame, _area)
    }

    pub(crate) fn handle_key_event(&mut self, key: crossterm::event::KeyEvent) -> bool {
        self.inner.handle_key_event(key)
    }

    pub(crate) fn finish(self) -> Result<T> {
        if let Some((_, value)) = self.inner.finish() {
            T::from_str(&value).map_err(|_| eyre!("Failed to parse enum value {value:?}"))
        } else {
            bail!("No value selected")
        }
    }
}
