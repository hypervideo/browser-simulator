use chromiumoxide::cdp::browser_protocol::network::CookieParam;
use chrono::prelude::*;
use eyre::{
    Context as _,
    OptionExt as _,
    Result,
};

/// A token (actually a cookie) to authenticate against the hyper.video server.
#[derive(Clone, Debug)]
pub(crate) struct AuthToken {
    #[expect(unused)]
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    cookie: String,
}

impl AuthToken {
    pub(crate) fn new(cookie: impl ToString) -> Self {
        let created_at = Utc::now();
        Self {
            created_at,
            // TODO: Currently we use a year expiration date on the server but we should dynamically determine this
            // value here as it is likely to change.
            expires_at: created_at + chrono::Duration::days(365),
            cookie: cookie.to_string(),
        }
    }

    #[expect(unused)]
    pub(crate) fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    fn cookie_header(&self) -> reqwest::header::HeaderValue {
        reqwest::header::HeaderValue::from_str(&format!("hyper_session={}", self.cookie))
            .expect("failed to create cookie header")
    }

    pub(crate) async fn fetch_token(server_base_url: impl AsRef<str>) -> Result<Self> {
        let server_base_url = server_base_url.as_ref();

        debug!(?server_base_url, "Requesting guest cookie");

        let response = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()?
            .post(format!("{server_base_url}/api/v1/auth/guest"))
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

        Ok(Self::new(cookie))
    }

    #[expect(unused)]
    pub(crate) async fn check_validity(&self, server_base_url: &str) -> bool {
        reqwest::Client::new()
            .get(format!("{server_base_url}/api/v1/auth/me"))
            .header("Cookie", self.cookie_header())
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
            .header("Cookie", self.cookie_header())
            .body("{}")
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub(crate) async fn set_name(&mut self, name: impl AsRef<str>, server_base_url: &str) -> Result<()> {
        reqwest::Client::new()
            .put(format!("{server_base_url}/api/v1/auth/me/name"))
            .header("Content-Type", "application/json")
            .header("Cookie", self.cookie_header())
            .json(&serde_json::json!({
                "name": name.as_ref(),
            }))
            .send()
            .await?
            .error_for_status()?;

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
