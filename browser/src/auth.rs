use base64::{
    engine::general_purpose::URL_SAFE_NO_PAD,
    Engine as _,
};
use eyre::{
    ensure,
    Context as _,
    OptionExt as _,
    Result,
};
use serde::{
    Deserialize,
    Serialize,
};
use std::{
    collections::{
        HashMap,
        VecDeque,
    },
    path::{
        Path,
        PathBuf,
    },
    sync::{
        Arc,
        Mutex,
    },
};

/// Reuses guest credentials between simulator participants without sharing one identity concurrently.
#[derive(Clone, Debug)]
pub struct FirstPartyCredentialsManager {
    stash_file: PathBuf,
    // ponytail: the pool lock also serializes small stash writes; split it if disk I/O causes contention.
    available_credentials: Arc<Mutex<HashMap<String, VecDeque<FirstPartyCredentials>>>>,
}

impl FirstPartyCredentialsManager {
    pub fn new(stash_file: impl Into<PathBuf>) -> Self {
        Self {
            stash_file: stash_file.into(),
            available_credentials: Default::default(),
        }
    }

    pub fn give_credentials(&self, base_url: &url::Url) -> Option<BorrowedCredentials> {
        let server_url = server_key(base_url);
        let mut available = self.available_credentials.lock().unwrap();
        let credentials = available.entry(server_url.clone()).or_default();
        credentials.retain(FirstPartyCredentials::can_renew);
        credentials
            .pop_front()
            .map(|credentials| BorrowedCredentials::new(server_url, credentials, self.clone()))
            .inspect(|credentials| {
                debug!(name = credentials.username(), "borrowed first-party credentials");
            })
    }

    fn return_credentials(&self, server_url: String, credentials: FirstPartyCredentials) {
        if !credentials.can_renew() {
            return;
        }
        debug!(name = credentials.username, "returned first-party credentials");
        self.available_credentials
            .lock()
            .unwrap()
            .entry(server_url)
            .or_default()
            .push_back(credentials);
    }

    pub async fn fetch_new_credentials(
        &self,
        base_url: url::Url,
        username: impl AsRef<str>,
    ) -> Result<BorrowedCredentials> {
        let credentials = FirstPartyCredentials::fetch(&base_url, username).await?;
        let server_url = server_key(&base_url);

        let _available = self.available_credentials.lock().unwrap();
        let mut stash = FirstPartyCredentialsStash::load(&self.stash_file);
        stash
            .credentials
            .entry(server_url.clone())
            .or_default()
            .push(credentials.clone());
        stash.save()?;

        Ok(BorrowedCredentials::new(server_url, credentials, self.clone()))
    }

    pub async fn give_or_fetch_credentials(
        &self,
        base_url: url::Url,
        username: impl AsRef<str>,
    ) -> Result<BorrowedCredentials> {
        if let Some(credentials) = self.give_credentials(&base_url) {
            return Ok(credentials);
        }

        self.fetch_new_credentials(base_url, username).await
    }
}

impl From<FirstPartyCredentialsStash> for FirstPartyCredentialsManager {
    fn from(stash: FirstPartyCredentialsStash) -> Self {
        Self {
            stash_file: stash.stash_file,
            available_credentials: Arc::new(Mutex::new(
                stash
                    .credentials
                    .into_iter()
                    .map(|(server_url, credentials)| (server_url, VecDeque::from(credentials)))
                    .collect(),
            )),
        }
    }
}

/// Credentials that return to the manager when their participant stops.
#[derive(Debug)]
pub struct BorrowedCredentials {
    server_url: String,
    credentials: FirstPartyCredentials,
    manager: FirstPartyCredentialsManager,
}

impl Drop for BorrowedCredentials {
    fn drop(&mut self) {
        self.manager
            .return_credentials(self.server_url.clone(), self.credentials.clone());
    }
}

impl BorrowedCredentials {
    fn new(server_url: String, credentials: FirstPartyCredentials, manager: FirstPartyCredentialsManager) -> Self {
        Self {
            server_url,
            credentials,
            manager,
        }
    }

    pub fn username(&self) -> &str {
        &self.credentials.username
    }

    pub fn realm(&self) -> &str {
        &self.credentials.realm
    }

    pub fn envelope_json(&self) -> Result<String> {
        self.credentials.envelope_json()
    }

    /// Keep browser renewals for the next participant and the next simulator run.
    pub(crate) fn update_from_envelope(&mut self, envelope: &str) -> Result<()> {
        #[derive(Deserialize)]
        struct Envelope {
            realm: String,
            first_party_access_token: String,
            renewal_token: String,
        }
        let envelope: Envelope = serde_json::from_str(envelope).context("invalid browser credentials")?;
        ensure!(envelope.realm == self.server_url, "browser credential realm mismatch");
        let credentials = FirstPartyCredentials {
            username: self.credentials.username.clone(),
            realm: envelope.realm,
            first_party_access_token: envelope.first_party_access_token,
            renewal_token: envelope.renewal_token,
        };
        ensure!(
            !credentials.first_party_access_token.is_empty() && credentials.can_renew(),
            "browser credentials are empty or cannot be renewed"
        );
        if credentials.first_party_access_token == self.credentials.first_party_access_token
            && credentials.renewal_token == self.credentials.renewal_token
        {
            return Ok(());
        }

        // Retain the renewed pair in memory even if persisting it fails.
        let previous = std::mem::replace(&mut self.credentials, credentials);
        let _available = self.manager.available_credentials.lock().unwrap();
        let mut stash = FirstPartyCredentialsStash::load(&self.manager.stash_file);
        let stored = stash.credentials.entry(self.server_url.clone()).or_default();
        stored.retain(|entry| entry.renewal_token != previous.renewal_token);
        stored.push(self.credentials.clone());
        stash.save()
    }
}

fn server_key(base_url: &url::Url) -> String {
    base_url.origin().ascii_serialization()
}

/// Persist guest identities only where creating a new account for every run is undesirable.
const PERSISTENCE_WHITELIST: [&str; 3] = ["latest.dev.hyper.video", "staging.hyper.video", "meet.hyper.video"];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct FirstPartyCredentialsStash {
    #[serde(skip)]
    stash_file: PathBuf,
    credentials: HashMap<String, Vec<FirstPartyCredentials>>,
}

impl FirstPartyCredentialsStash {
    fn load(file: impl AsRef<Path>) -> Self {
        let file = file.as_ref();
        let mut stash: Self = file
            .exists()
            .then(|| {
                std::fs::File::open(file)
                    .ok()
                    .and_then(|file| serde_json::from_reader(file).ok())
            })
            .flatten()
            .inspect(|_| debug!(?file, "loaded first-party credentials"))
            .unwrap_or_else(|| {
                debug!(?file, "no first-party credentials found");
                Self {
                    stash_file: file.to_path_buf(),
                    credentials: Default::default(),
                }
            });
        stash.stash_file = file.to_path_buf();
        for credentials in stash.credentials.values_mut() {
            credentials.retain(FirstPartyCredentials::can_renew);
        }
        stash
    }

    pub(crate) fn load_from_data_dir(data_dir: impl AsRef<Path>) -> Self {
        Self::load(data_dir.as_ref().join("first_party_credentials.json"))
    }

    fn with_whitelisted_servers(&self) -> Self {
        let credentials = self
            .credentials
            .iter()
            .filter(|(server_url, _)| {
                url::Url::parse(server_url)
                    .ok()
                    .and_then(|url| url.host_str().map(str::to_owned))
                    .is_some_and(|host| PERSISTENCE_WHITELIST.contains(&host.as_str()))
            })
            .map(|(server_url, credentials)| (server_url.clone(), credentials.clone()))
            .collect();

        Self {
            stash_file: self.stash_file.clone(),
            credentials,
        }
    }

    fn save(&self) -> Result<()> {
        let dir = self.stash_file.parent().ok_or_eyre("failed to get parent directory")?;
        std::fs::create_dir_all(dir)?;
        let mut options = std::fs::OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let file = options.open(&self.stash_file)?;
        serde_json::to_writer_pretty(&file, &self.with_whitelisted_servers())?;
        debug!(?file, "saved first-party credentials");
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct FirstPartyCredentials {
    username: String,
    realm: String,
    first_party_access_token: String,
    renewal_token: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GuestAuthResponse {
    first_party_access_token: String,
    renewal_token: String,
}

impl FirstPartyCredentials {
    fn can_renew(&self) -> bool {
        #[derive(Deserialize)]
        struct Expiry {
            exp: i64,
        }

        // Cache eviction only: the server still verifies the JWT. An expired access
        // token is reusable as long as Hyper Core can renew it after startup.
        self.renewal_token
            .split('.')
            .nth(1)
            .and_then(|payload| URL_SAFE_NO_PAD.decode(payload).ok())
            .and_then(|payload| serde_json::from_slice::<Expiry>(&payload).ok())
            .is_some_and(|expiry| expiry.exp > chrono::Utc::now().timestamp())
    }

    async fn fetch(base_url: &url::Url, username: impl AsRef<str>) -> Result<Self> {
        let username = username.as_ref();
        let url = base_url
            .join("/api/v1/auth/guest")
            .context("failed to join base URL with /api/v1/auth/guest")?;

        debug!(%url, %username, "requesting guest credentials");
        let response = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .danger_accept_invalid_certs(true)
            .build()
            .context("failed to build reqwest client")?
            .post(url)
            .query(&[("username", username)])
            .send()
            .await?
            .error_for_status()?
            .json::<GuestAuthResponse>()
            .await?;

        Ok(Self {
            username: username.to_owned(),
            realm: base_url.origin().ascii_serialization(),
            first_party_access_token: response.first_party_access_token,
            renewal_token: response.renewal_token,
        })
    }

    fn envelope_json(&self) -> Result<String> {
        serde_json::to_string(&serde_json::json!({
            "realm": self.realm,
            "first_party_access_token": self.first_party_access_token,
            "renewal_token": self.renewal_token,
        }))
        .context("failed to serialize first-party credentials")
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::time::{
        SystemTime,
        UNIX_EPOCH,
    };
    use tokio::{
        io::{
            AsyncReadExt as _,
            AsyncWriteExt as _,
        },
        net::TcpListener,
    };

    const SERVER_URL: &str = "https://staging.hyper.video";

    fn credentials(username: &str) -> FirstPartyCredentials {
        FirstPartyCredentials {
            username: username.to_string(),
            realm: "https://staging.hyper.video".to_string(),
            first_party_access_token: format!("{username}-access"),
            renewal_token: renewal_token(i64::MAX),
        }
    }

    fn renewal_token(exp: i64) -> String {
        let payload = URL_SAFE_NO_PAD.encode(serde_json::json!({ "exp": exp }).to_string());
        format!("e30.{payload}.signature")
    }

    #[tokio::test]
    async fn lite_and_stub_participants_do_not_reuse_core_names() {
        use crate::participant::ParticipantStore;
        use client_simulator_config::{
            Config,
            ParticipantBackendKind,
        };

        for (backend, path) in [
            (ParticipantBackendKind::Local, "/m/demo"),
            (ParticipantBackendKind::RemoteStub, "/demo"),
        ] {
            let dir = unique_temp_dir();
            stash_with(
                dir.join("first_party_credentials.json"),
                vec![credentials("cached-core")],
            )
            .save()
            .unwrap();
            let store = ParticipantStore::new(&dir);
            let config = Config {
                backend,
                url: Some(format!("{SERVER_URL}{path}").parse().unwrap()),
                ..Default::default()
            };
            // Both spawns finish synchronously, before either browser task starts.
            store.spawn(&config).unwrap();
            store.spawn(&config).unwrap();
            assert_eq!(store.len(), 2);
            assert!(store.keys().iter().all(|name| name != "cached-core"));
            assert_eq!(
                store
                    .credentials()
                    .give_credentials(config.url.as_ref().unwrap())
                    .unwrap()
                    .username(),
                "cached-core"
            );
            store.shutdown_all().await;
        }
    }

    #[test]
    fn pool_discards_expired_and_unreadable_renewal_tokens() {
        let server_url = url::Url::parse(SERVER_URL).unwrap();
        for token in [
            renewal_token(0),
            renewal_token(chrono::Utc::now().timestamp()),
            "invalid".to_owned(),
            "e30.e30.signature".to_owned(),
        ] {
            let mut expired = credentials("expired");
            expired.renewal_token = token;
            let mut renewable = credentials("usable");
            renewable.first_party_access_token = renewal_token(0);
            let manager =
                FirstPartyCredentialsManager::from(stash_with("unused.json".into(), vec![expired.clone(), renewable]));
            // An expired access token is still reusable while renewal remains possible.
            let usable = manager.give_credentials(&server_url).unwrap();
            assert_eq!(usable.username(), "usable");
            manager.return_credentials(SERVER_URL.to_owned(), expired);
            assert!(manager.give_credentials(&server_url).is_none());
        }
    }

    #[test]
    fn stash_prunes_expired_credentials_before_saving_replacements() {
        let stash_file = unique_temp_dir().join("first_party_credentials.json");
        let mut expired = credentials("expired");
        expired.renewal_token = renewal_token(0);
        stash_with(stash_file.clone(), vec![expired, credentials("usable")])
            .save()
            .unwrap();
        let loaded = FirstPartyCredentialsStash::load(&stash_file);
        assert_eq!(loaded.credentials[SERVER_URL].len(), 1);
        loaded.save().unwrap();
        let stored = std::fs::read_to_string(stash_file).unwrap();
        assert!(!stored.contains("expired"));
        assert!(stored.contains("usable"));
    }

    fn stash_with(stash_file: PathBuf, credentials: Vec<FirstPartyCredentials>) -> FirstPartyCredentialsStash {
        FirstPartyCredentialsStash {
            stash_file,
            credentials: HashMap::from([(SERVER_URL.to_string(), credentials)]),
        }
    }

    fn unique_temp_dir() -> PathBuf {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("hyper-browser-simulator-auth-{nonce}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub(crate) fn borrowed_for_test() -> (BorrowedCredentials, FirstPartyCredentialsManager) {
        let stash = stash_with(
            unique_temp_dir().join("first_party_credentials.json"),
            vec![credentials("simulator")],
        );
        stash.save().unwrap();
        let manager = FirstPartyCredentialsManager::from(stash);
        (manager.give_credentials(&SERVER_URL.parse().unwrap()).unwrap(), manager)
    }

    #[test]
    fn browser_renewals_replace_the_original_pair_in_pool_and_stash() {
        for original_expired in [false, true] {
            let (mut borrowed, manager) = borrowed_for_test();
            if original_expired {
                // The original pair expires while the browser has already renewed it.
                borrowed.credentials.renewal_token = renewal_token(0);
            }
            let mut other = credentials("simulator");
            other.renewal_token = renewal_token(i64::MAX - 1);
            stash_with(
                manager.stash_file.clone(),
                vec![borrowed.credentials.clone(), other.clone()],
            )
            .save()
            .unwrap();
            let mut renewed = credentials("simulator");
            renewed.first_party_access_token = "renewed-access".to_string();
            renewed.renewal_token = renewal_token(i64::MAX - 2);
            let envelope = renewed.envelope_json().unwrap();
            borrowed.update_from_envelope(&envelope).unwrap();
            borrowed.update_from_envelope(&envelope).unwrap();
            drop(borrowed);

            let reused = manager.give_credentials(&SERVER_URL.parse().unwrap()).unwrap();
            assert_eq!(reused.envelope_json().unwrap(), envelope);
            let loaded = FirstPartyCredentialsStash::load(&manager.stash_file);
            let stored = &loaded.credentials[SERVER_URL];
            assert_eq!(stored.len(), 2);
            assert_eq!(stored[0].renewal_token, other.renewal_token);
            assert_eq!(stored[1].envelope_json().unwrap(), envelope);
        }
    }

    #[test]
    fn invalid_browser_credentials_do_not_replace_the_borrowed_pair() {
        let (mut borrowed, manager) = borrowed_for_test();
        let original = borrowed.envelope_json().unwrap();
        let stored = std::fs::read(&manager.stash_file).unwrap();
        for envelope in [
            "not json".to_string(),
            "{}".to_string(),
            original.replace(SERVER_URL, "https://other.example"),
            original.replace("simulator-access", ""),
            original.replace(&renewal_token(i64::MAX), &renewal_token(0)),
            original.replace(&renewal_token(i64::MAX), "invalid"),
        ] {
            assert!(borrowed.update_from_envelope(&envelope).is_err());
            assert_eq!(borrowed.envelope_json().unwrap(), original);
            assert_eq!(std::fs::read(&manager.stash_file).unwrap(), stored);
        }
    }

    #[test]
    fn persistence_failure_still_returns_browser_renewals_to_the_pool() {
        let (mut borrowed, manager) = borrowed_for_test();
        // An existing directory cannot be opened as a stash file for writing.
        borrowed.manager.stash_file = unique_temp_dir();
        let envelope = borrowed
            .envelope_json()
            .unwrap()
            .replace("simulator-access", "renewed-access");
        assert!(borrowed.update_from_envelope(&envelope).is_err());
        drop(borrowed);
        assert_eq!(
            manager
                .give_credentials(&SERVER_URL.parse().unwrap())
                .unwrap()
                .envelope_json()
                .unwrap(),
            envelope
        );
    }

    #[test]
    fn borrowed_credentials_return_to_the_pool() {
        let manager =
            FirstPartyCredentialsManager::from(stash_with("unused.json".into(), vec![credentials("simulator")]));

        let server_url = url::Url::parse(SERVER_URL).unwrap();
        let borrowed = manager.give_credentials(&server_url).unwrap();
        assert_eq!(borrowed.username(), "simulator");
        assert!(manager.give_credentials(&server_url).is_none());
        drop(borrowed);

        let mut borrowed = manager.give_credentials(&server_url).unwrap();
        assert_eq!(borrowed.username(), "simulator");
        // Simulate the renewal token expiring while the participant is running.
        borrowed.credentials.renewal_token = renewal_token(0);
        drop(borrowed);
        assert!(manager.give_credentials(&server_url).is_none());
    }

    #[test]
    fn stash_round_trips_credentials() {
        let stash_file = unique_temp_dir().join("first_party_credentials.json");
        stash_with(stash_file.clone(), vec![credentials("simulator")])
            .save()
            .unwrap();

        let stored = std::fs::read_to_string(&stash_file).unwrap();
        assert!(!stored.contains("stash_file"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                std::fs::metadata(&stash_file).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }

        let loaded = FirstPartyCredentialsStash::load(stash_file);

        assert_eq!(loaded.credentials[SERVER_URL][0].username, "simulator");
    }

    #[test]
    fn envelope_matches_hyper_core_storage_shape() {
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&credentials("simulator").envelope_json().unwrap()).unwrap(),
            serde_json::json!({
                "realm": "https://staging.hyper.video",
                "first_party_access_token": "simulator-access",
                "renewal_token": renewal_token(i64::MAX),
            })
        );
    }

    #[tokio::test]
    async fn expired_pool_entry_is_replaced_and_the_fresh_credentials_are_reused() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = url::Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let mut expired = credentials("expired");
        expired.renewal_token = renewal_token(0);
        let manager = FirstPartyCredentialsManager::from(FirstPartyCredentialsStash {
            stash_file: unique_temp_dir().join("first_party_credentials.json"),
            credentials: HashMap::from([(server_key(&base_url), vec![expired])]),
        });
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0_u8; 4096];
            let bytes_read = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..bytes_read]);
            assert!(request.starts_with("POST /api/v1/auth/guest?username=simulator HTTP/1.1"));

            let body = serde_json::json!({
                "firstPartyAccessToken": "access",
                "renewalToken": renewal_token(i64::MAX),
            })
            .to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let borrowed = manager
            .give_or_fetch_credentials(base_url.clone(), "simulator")
            .await
            .unwrap();
        server.await.unwrap();

        let credentials = &borrowed.credentials;
        assert_eq!(credentials.username, "simulator");
        assert_eq!(credentials.realm, base_url.origin().ascii_serialization());
        assert_eq!(credentials.first_party_access_token, "access");
        assert_eq!(credentials.renewal_token, renewal_token(i64::MAX));
        drop(borrowed);
        // The mock server has stopped: reuse must not make another HTTP request.
        let reused = manager.give_or_fetch_credentials(base_url, "simulator").await.unwrap();
        assert_eq!(reused.credentials.first_party_access_token, "access");
    }
}
