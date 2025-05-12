use crate::config::BrowserConfig;
use chromiumoxide::{
    browser::{
        Browser,
        BrowserConfig as OxideBrowserConfig,
    },
    cdp::browser_protocol::{
        network::CookieParam,
        target::CreateTargetParams,
    },
};
use eyre::{
    Context as _,
    Result,
};
use futures::StreamExt as _;
use std::{
    path::PathBuf,
    time::Duration,
};
use url::Url;

fn get_binary() -> Result<PathBuf> {
    if let Ok(chromium) = which::which("chromium") {
        debug!(?chromium, "found chromium at");
        return Ok(chromium);
    } else {
        debug!("chromium not found, falling back to google-chrome");
    }

    let chrome = which::which("google-chrome").context("google chrome not found")?;
    debug!(?chrome, "found google-chrome at");
    Ok(chrome)
}

pub struct Join {
    browser_config: BrowserConfig,
}

impl Join {
    pub fn new(browser_config: BrowserConfig) -> Self {
        Self { browser_config }
    }

    pub async fn run(&self) -> Result<()> {
        let url = Url::parse(&self.browser_config.url).context("failed to parse url")?;

        let binary = get_binary().expect("binary not found");

        // Build browser config with fake media args
        let mut chrome_args = Vec::new();
        if self.browser_config.fake_media {
            chrome_args.extend([
                "--use-fake-ui-for-media-stream".to_string(),
                "--use-fake-device-for-media-stream".to_string(),
            ]);
            if let Some(path) = &self.browser_config.fake_video_file {
                chrome_args.push(format!("--use-file-for-fake-video-capture={}", path));
            }
        }

        let browser_config = OxideBrowserConfig::builder()
            .with_head()
            .user_data_dir(format!("./tmp/chromiumoxide-{}", self.browser_config.instance_id))
            .chrome_executable(binary)
            .args(&chrome_args)
            .build()
            .map_err(|e| eyre::eyre!(e))
            .context("failed to build browser config")?;

        // Launch browser
        let (mut browser, mut handler) = Browser::launch(browser_config)
            .await
            .context("failed to launch browser")?;

        let handle = tokio::task::spawn(async move {
            while let Some(event) = handler.next().await {
                if let Err(err) = event {
                    error!("error in browser handler: {err:?}");
                }
            }
        });

        debug!(url = &self.browser_config.url, "opening url");

        // Create page
        let page = browser
            .new_page(
                CreateTargetParams::builder()
                    .url(&self.browser_config.url)
                    .build()
                    .map_err(|e| eyre::eyre!(e))?,
            )
            .await
            .context("failed to create new page")?;

        // Set cookie if provided
        if !self.browser_config.cookie.is_empty() {
            let cookie = CookieParam::builder()
                .name("hyper_session")
                .value(self.browser_config.cookie.clone())
                .domain(url.host_str().unwrap_or("localhost"))
                .path("/")
                .build()
                .map_err(|e| eyre::eyre!(e))
                .context("failed to build cookie")?;
            page.set_cookies(vec![cookie]).await.context("failed to set cookie")?;
        }

        page.wait_for_navigation_response()
            .await
            .context("failed to wait for navigation response")?;

        debug!("page loaded, joining session");

        // Find the join button and click it
        let el = loop {
            match page.find_element(r#"button[type="submit"]"#).await {
                Ok(el) => break el,
                Err(chromiumoxide::error::CdpError::Chrome(err)) if err.to_string().contains("Could not find node") => {
                }
                Err(chromiumoxide::error::CdpError::NotFound) => {}
                Err(e) => {
                    error!("failed to find join button: {e:?}");
                    return Err(e.into());
                }
            }
            debug!("join button not found, retrying");
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        el.click().await.context("failed to click submit button")?;

        debug!("waiting for browser to exit");

        // Wait for browser to exit
        browser.wait().await.context("failed to wait for browser")?;

        handle.await.context("failed to wait for browser event loop")?;

        Ok(())
    }
}
