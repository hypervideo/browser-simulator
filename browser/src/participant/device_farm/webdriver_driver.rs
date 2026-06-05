use crate::participant::frontend::BrowserDriver;
use eyre::{
    Context as _,
    Result,
};
use futures::{
    future::BoxFuture,
    FutureExt as _,
};
use std::time::Duration;
use thirtyfour::{
    By,
    Cookie,
    WebDriver,
};

/// `BrowserDriver` implementation backed by a remote Selenium `WebDriver`
/// (AWS Device Farm Test Grid endpoint).
#[allow(dead_code)]
pub(crate) struct WebDriverDriver {
    driver: WebDriver,
}

#[allow(dead_code)]
impl WebDriverDriver {
    pub(crate) fn new(driver: WebDriver) -> Self {
        Self { driver }
    }

    /// Consume the driver and end the remote session.
    pub(crate) async fn quit(self) -> Result<()> {
        self.driver.quit().await.context("failed to quit WebDriver session")
    }

    /// A cheap no-op WebDriver command, used by the keep-alive poller to stay
    /// inside `aws:idleTimeoutSecs` and to detect a dead session.
    pub(crate) async fn ping(&self) -> Result<()> {
        self.driver
            .current_url()
            .await
            .context("WebDriver session is not responsive")?;
        Ok(())
    }
}

impl BrowserDriver for WebDriverDriver {
    fn goto(&self, url: &str) -> BoxFuture<'_, Result<()>> {
        let url = url.to_owned();
        async move { self.driver.goto(&url).await.context("failed to navigate") }.boxed()
    }

    fn exists(&self, selector: &str) -> BoxFuture<'_, Result<bool>> {
        let selector = selector.to_owned();
        async move { Ok(self.driver.find(By::Css(&selector)).await.is_ok()) }.boxed()
    }

    fn wait_for(&self, selector: &str, timeout: Duration) -> BoxFuture<'_, Result<()>> {
        let selector = selector.to_owned();
        async move {
            let start = std::time::Instant::now();
            loop {
                if self.driver.find(By::Css(&selector)).await.is_ok() {
                    return Ok(());
                }
                if start.elapsed() > timeout {
                    eyre::bail!("timeout waiting for selector {selector}");
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
        .boxed()
    }

    fn click(&self, selector: &str) -> BoxFuture<'_, Result<()>> {
        let selector = selector.to_owned();
        async move {
            self.driver
                .find(By::Css(&selector))
                .await
                .with_context(|| format!("Could not find {selector}"))?
                .click()
                .await
                .with_context(|| format!("Could not click {selector}"))
        }
        .boxed()
    }

    fn fill(&self, selector: &str, text: &str) -> BoxFuture<'_, Result<()>> {
        let selector = selector.to_owned();
        let text = text.to_owned();
        async move {
            let element = self
                .driver
                .find(By::Css(&selector))
                .await
                .with_context(|| format!("Could not find {selector}"))?;
            element.clear().await.context("failed to clear element")?;
            element.send_keys(&text).await.context("failed to type into element")?;
            Ok(())
        }
        .boxed()
    }

    fn attribute(&self, selector: &str, name: &str) -> BoxFuture<'_, Result<Option<String>>> {
        let selector = selector.to_owned();
        let name = name.to_owned();
        async move {
            match self.driver.find(By::Css(&selector)).await {
                Ok(element) => element
                    .attr(&name)
                    .await
                    .with_context(|| format!("failed to read attribute {name}")),
                Err(_) => Ok(None),
            }
        }
        .boxed()
    }

    fn eval(&self, js_body: &str, arg: Option<serde_json::Value>) -> BoxFuture<'_, Result<serde_json::Value>> {
        let js_body = js_body.to_owned();
        async move {
            let args = match arg {
                Some(value) => vec![value],
                None => vec![],
            };
            let ret = self
                .driver
                .execute(&js_body, args)
                .await
                .context("failed to execute script")?;
            Ok(ret.json().clone())
        }
        .boxed()
    }

    fn set_cookie(&self, domain: &str, name: &str, value: &str) -> BoxFuture<'_, Result<()>> {
        let domain = domain.to_owned();
        let name = name.to_owned();
        let value = value.to_owned();
        async move {
            // WebDriver requires being on the target origin before adding a cookie.
            let origin = format!("https://{domain}/");
            self.driver
                .goto(&origin)
                .await
                .with_context(|| format!("failed to open origin {origin} before setting cookie"))?;
            let mut cookie = Cookie::new(name, value);
            cookie.set_domain(domain);
            cookie.set_path("/");
            self.driver.add_cookie(cookie).await.context("failed to add cookie")?;
            Ok(())
        }
        .boxed()
    }
}
