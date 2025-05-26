use chromiumoxide::cdp::browser_protocol::network::CookieParam;
use chrono::prelude::*;
use eyre::{
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

/// Manages cookies. Provides access to borrowed cookies.
#[derive(Clone, Debug)]
pub struct HyperSessionCookieManger {
    stash_file: PathBuf,
    available_cookies: Arc<Mutex<HashMap<Domain, VecDeque<HyperSessionCookie>>>>,
}

impl HyperSessionCookieManger {
    pub fn new(stash_file: impl Into<PathBuf>) -> Self {
        Self {
            stash_file: stash_file.into(),
            available_cookies: Default::default(),
        }
    }

    pub fn give_cookie(&self, domain: impl ToString) -> Option<BorrowedCookie> {
        let domain = domain.to_string();
        let mut available_cookies = self.available_cookies.lock().unwrap();
        let available_cookies = available_cookies.entry(domain.clone()).or_default();
        available_cookies
            .pop_front()
            .map(|cookie| BorrowedCookie::new(domain, cookie, self.clone()))
            .inspect(|cookie| {
                debug!(name = cookie.username(), "borrowed cookie");
            })
    }

    fn return_cookie(&self, domain: impl ToString, cookie: HyperSessionCookie) {
        debug!(name = cookie.username, "returned cookie");
        let mut available_cookies = self.available_cookies.lock().unwrap();
        let available_cookies = available_cookies.entry(domain.to_string()).or_default();
        available_cookies.push_back(cookie);
    }

    pub async fn fetch_new_cookie(&self, base_url: impl ToString, username: impl AsRef<str>) -> Result<BorrowedCookie> {
        let base_url = base_url.to_string();
        let cookie = HyperSessionCookie::fetch_token_and_set_name(&base_url, username).await?;

        // Safe the new cookie so we can reuse it later.
        let mut stash = HyperSessionCookieStash::load(&self.stash_file);
        stash.cookies.entry(base_url.clone()).or_default().push(cookie.clone());
        stash.save()?;

        Ok(BorrowedCookie::new(base_url, cookie, self.clone()))
    }
}

impl From<HyperSessionCookieStash> for HyperSessionCookieManger {
    fn from(stash: HyperSessionCookieStash) -> Self {
        Self {
            stash_file: stash.stash_file,
            available_cookies: Arc::new(Mutex::new(
                stash
                    .cookies
                    .into_iter()
                    .map(|(domain, cookies)| (domain, VecDeque::from(cookies)))
                    .collect(),
            )),
        }
    }
}

// -=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-

/// A cookie that will be returned to the manager when dropped.
#[derive(Debug)]
pub struct BorrowedCookie {
    domain: Domain,
    pub(crate) cookie: HyperSessionCookie,
    manager: HyperSessionCookieManger,
}

impl Drop for BorrowedCookie {
    fn drop(&mut self) {
        self.manager.return_cookie(self.domain.clone(), self.cookie.clone());
    }
}

impl BorrowedCookie {
    pub(crate) fn new(domain: impl ToString, cookie: HyperSessionCookie, manager: HyperSessionCookieManger) -> Self {
        Self {
            domain: domain.to_string(),
            cookie,
            manager,
        }
    }

    pub(crate) fn as_browser_cookie_for(&self, domain: impl AsRef<str>) -> Result<CookieParam> {
        self.cookie.as_browser_cookie_for(domain)
    }

    pub(crate) fn username(&self) -> &str {
        &self.cookie.username
    }
}

// -=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-

pub type Domain = String;

/// Only store cookies for selected hyper servers. For these servers we don't want to needlessly create new guest
/// accounts, for other (dev) servers guest creation does not matter.
const PERSISTENCE_WHITELIST: [&str; 3] = ["latest.dev.hyper.video", "staging.hyper.video", "meet.hyper.video"];

/// List of cookies that can be stored and retrieved.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct HyperSessionCookieStash {
    stash_file: PathBuf,
    cookies: HashMap<Domain, Vec<HyperSessionCookie>>,
}

impl HyperSessionCookieStash {
    /// Load the cookies from the given directory.
    fn load(file: impl AsRef<Path>) -> Self {
        let file = file.as_ref();
        file.exists()
            .then(|| {
                std::fs::File::open(file)
                    .ok()
                    .and_then(|f| serde_json::from_reader(f).ok())
            })
            .flatten()
            .inspect(|_| {
                debug!(?file, "loaded hyper_session cookies");
            })
            .unwrap_or_else(|| {
                debug!(?file, "no hyper_session cookies found");
                Self {
                    stash_file: file.to_path_buf(),
                    cookies: Default::default(),
                }
            })
    }

    /// Load the cookies from the given directory.
    pub(super) fn load_from_data_dir(data_dir: impl AsRef<Path>) -> Self {
        const HYPER_COOKIES_FILE: &str = "hyper_session_cookies.json";
        let file = data_dir.as_ref().join(HYPER_COOKIES_FILE);
        Self::load(file)
    }

    fn with_whitelisted_domains(&self) -> Self {
        let cookies = self
            .cookies
            .iter()
            .filter(|(domain, _)| {
                PERSISTENCE_WHITELIST
                    .iter()
                    .any(|whitelisted| domain.contains(whitelisted))
            })
            .map(|(domain, cookies)| (domain.clone(), cookies.clone()))
            .collect();

        Self {
            stash_file: self.stash_file.clone(),
            cookies,
        }
    }

    /// Save the cookies to the given directory.
    fn save(&self) -> Result<()> {
        let dir = self.stash_file.parent().ok_or_eyre("failed to get parent directory")?;
        std::fs::create_dir_all(dir)?;
        let file = std::fs::File::create(&self.stash_file)?;
        serde_json::to_writer_pretty(&file, &self.with_whitelisted_domains())?;
        debug!(?file, "saved hyper_session cookies");
        Ok(())
    }
}

/// A token (actually a cookie) to authenticate against the hyper.video server.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct HyperSessionCookie {
    domain: Domain,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    pub(crate) username: String,
    cookie: String,
}

impl HyperSessionCookie {
    pub(crate) fn new(domain: impl ToString, cookie: impl ToString) -> Self {
        let created_at = Utc::now();
        Self {
            domain: domain.to_string(),
            created_at,
            // TODO: Currently we use a year expiration date on the server but we should dynamically determine this
            // value here as it is likely to change.
            expires_at: created_at + chrono::Duration::days(365),
            username: Default::default(),
            cookie: cookie.to_string(),
        }
    }

    #[expect(unused)]
    pub(crate) fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    fn cookie_header(&self) -> Result<reqwest::header::HeaderValue> {
        reqwest::header::HeaderValue::from_str(&format!("hyper_session={}", self.cookie))
            .context("failed to create cookie header")
    }

    async fn fetch_token_and_set_name(base_url: impl AsRef<str>, name: impl AsRef<str>) -> Result<Self> {
        let base_url = base_url.as_ref();
        let mut auth = HyperSessionCookie::fetch_token(base_url).await?;
        auth.set_name(name, base_url).await?;
        Ok(auth)
    }

    async fn fetch_token(base_url: impl AsRef<str>) -> Result<Self> {
        let base_url = base_url.as_ref();

        debug!(?base_url, "Requesting guest cookie");

        let response = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()?
            .post(format!("{base_url}/api/v1/auth/guest"))
            .query(&[("username", "guest")])
            .send()
            .await?
            .error_for_status()?;

        let cookie = response
            .cookies()
            .find(|cookie| cookie.name() == "hyper_session")
            .ok_or_eyre("api/v1/auth/guest did not return a cookie")?
            .value()
            .to_string();

        Ok(Self::new(base_url, cookie))
    }

    #[expect(unused)]
    pub(crate) async fn check_validity(&self, server_base_url: &str) -> bool {
        let header = match self.cookie_header() {
            Ok(h) => h,
            _ => return false,
        };

        reqwest::Client::new()
            .get(format!("{server_base_url}/api/v1/auth/me"))
            .header("Cookie", header)
            .send()
            .await
            .map(|response| response.status().is_success())
            .unwrap_or(false)
    }

    #[expect(unused)]
    pub(crate) async fn logout(&self, server_base_url: &str) -> Result<()> {
        reqwest::Client::new()
            .post(format!("{server_base_url}/api/v1/auth/logout"))
            .header("Content-Type", "application/json")
            .header("Cookie", self.cookie_header()?)
            .body("{}")
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub(crate) async fn set_name(&mut self, name: impl AsRef<str>, server_base_url: &str) -> Result<()> {
        let name = name.as_ref();
        reqwest::Client::new()
            .put(format!("{server_base_url}/api/v1/auth/me/name"))
            .header("Content-Type", "application/json")
            .header("Cookie", self.cookie_header()?)
            .json(&serde_json::json!({
                "name": name,
            }))
            .send()
            .await?
            .error_for_status()?;
        self.username = name.to_string();
        Ok(())
    }

    pub(crate) fn as_browser_cookie_for(&self, domain: impl AsRef<str>) -> Result<CookieParam> {
        CookieParam::builder()
            .name("hyper_session")
            .value(self.cookie.clone())
            .domain(domain.as_ref())
            .path("/")
            .build()
            .map_err(|e| eyre::eyre!(e))
            .context("failed to build cookie")
    }
}
