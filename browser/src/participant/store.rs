use super::{
    HyperSessionCookieStash,
    Participant,
};
use crate::auth::HyperSessionCookieManger;
use client_simulator_config::Config;
use eyre::Result;
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        Arc,
        Mutex,
    },
    vec::IntoIter,
};

/// Store for all the participants that we will expose to the TUI
/// for displaying and control.
#[derive(Debug, Clone)]
pub struct ParticipantStore {
    cookies: HyperSessionCookieManger,
    inner: Arc<Mutex<HashMap<String, Participant>>>,
}

impl ParticipantStore {
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        Self {
            cookies: HyperSessionCookieStash::load_from_data_dir(data_dir).into(),
            inner: Default::default(),
        }
    }

    pub fn spawn_local(&self, config: &Config) -> Result<()> {
        let participant = Participant::spawn_with_app_config(config, self.cookies.clone())?;
        self.add(participant);
        Ok(())
    }

    pub fn spawn_remote(&self, config: &Config) -> Result<()> {
        let participant = Participant::spawn_remote(config, self.cookies.clone())?;
        self.add(participant);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock().unwrap().is_empty()
    }

    fn sorted(&self) -> IntoIter<Participant> {
        let mut participants = self.inner.lock().unwrap().values().cloned().collect::<Vec<_>>();
        participants.sort_by(|a, b| a.created.cmp(&b.created));

        participants.into_iter()
    }

    pub fn keys(&self) -> Vec<String> {
        self.sorted().map(|p| p.name.clone()).collect()
    }

    pub fn values(&self) -> Vec<Participant> {
        self.sorted().collect()
    }

    pub fn add(&self, participant: Participant) {
        self.inner.lock().unwrap().insert(participant.name.clone(), participant);
    }

    pub fn remove(&self, name: &str) -> Option<Participant> {
        self.inner.lock().unwrap().remove(name)
    }

    pub fn get(&self, name: &str) -> Option<Participant> {
        self.inner.lock().unwrap().get(name).cloned()
    }

    pub fn prev(&self, name: &str) -> Option<String> {
        let sorted = self.sorted().collect::<Vec<_>>();
        let index = sorted.iter().position(|p| p.name == name)?;
        (index > 0).then(|| sorted[index - 1].name.clone())
    }
}
