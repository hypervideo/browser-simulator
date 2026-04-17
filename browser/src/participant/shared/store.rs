use crate::{
    auth::{
        HyperSessionCookieManger,
        HyperSessionCookieStash,
    },
    participant::Participant,
};
use client_simulator_config::{
    Config,
    ParticipantBackendKind,
};
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

/// Store of active participants exposed to the TUI for display and control.
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

    pub fn cookies(&self) -> &HyperSessionCookieManger {
        &self.cookies
    }

    pub fn spawn(&self, config: &Config) -> Result<()> {
        let participant = Participant::spawn(config, self.cookies.clone())?;
        self.add(participant);
        Ok(())
    }

    pub fn spawn_local(&self, config: &Config) -> Result<()> {
        let mut config = config.clone();
        config.backend = ParticipantBackendKind::Local;
        self.spawn(&config)
    }

    pub fn spawn_remote_stub(&self, config: &Config) -> Result<()> {
        let mut config = config.clone();
        config.backend = ParticipantBackendKind::RemoteStub;
        self.spawn(&config)
    }

    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock().unwrap().is_empty()
    }

    fn sorted(&self) -> IntoIter<Participant> {
        let mut participants = self.inner.lock().unwrap().values().cloned().collect::<Vec<_>>();
        participants.sort_by_key(|a| a.created);

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

#[cfg(test)]
mod tests {
    use super::ParticipantStore;
    use crate::participant::cloudflare::take_spawned_participants_for_test;
    use client_simulator_config::{
        Config,
        ParticipantBackendKind,
    };
    use std::{
        fs,
        path::PathBuf,
        time::{
            SystemTime,
            UNIX_EPOCH,
        },
    };
    use url::Url;

    #[tokio::test]
    async fn spawn_dispatches_cloudflare_backend_to_cloudflare_constructor() {
        let _ = take_spawned_participants_for_test();

        let data_dir = unique_test_data_dir();
        fs::create_dir_all(&data_dir).expect("create temp data dir");

        let store = ParticipantStore::new(&data_dir);
        let config = Config {
            url: Some(Url::parse("https://example.com/space/demo").expect("valid url")),
            backend: ParticipantBackendKind::Cloudflare,
            ..Default::default()
        };

        store.spawn(&config).expect("spawn should dispatch");
        tokio::task::yield_now().await;

        let spawned = take_spawned_participants_for_test();
        assert_eq!(spawned.len(), 1);
        assert_eq!(spawned, store.keys());
    }

    fn unique_test_data_dir() -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("current time")
            .as_nanos();
        std::env::temp_dir().join(format!("hyper-browser-simulator-store-test-{timestamp}"))
    }
}
