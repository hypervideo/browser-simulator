use crate::config::BrowserConfig;
use chromiumoxide::{
    browser,
    Browser,
    Element,
    Handler,
    Page,
};
use eyre::{
    Context as _,
    Result,
};
use std::{
    path::PathBuf,
    time::Duration,
};

pub mod auth;
pub mod participant;

fn get_binary() -> Result<PathBuf> {
    // Chromium / Chrome can have different binary names
    let chrome = ["chromium", "google-chrome", "google-chrome-stable", "chrome"]
        .iter()
        .find_map(|name| {
            which::which(name).ok().map(|path| {
                debug!(?path, "found {} at", name);
                path
            })
        })
        .ok_or_else(|| eyre::eyre!("failed to find chromium or google-chrome binary"))?;
    debug!(?chrome, "chrome found at");
    Ok(chrome)
}

/// Create a new browser instance with the browser config.
async fn create_browser(browser_config: &BrowserConfig) -> Result<(Browser, Handler)> {
    let binary = get_binary()?;

    // Build browser config with fake media args
    let mut chrome_args = vec!["--no-startup-window".to_string()];
    if browser_config.fake_media {
        chrome_args.extend([
            "--use-fake-ui-for-media-stream".to_string(),
            "--use-fake-device-for-media-stream".to_string(),
        ]);
        if let Some(path) = &browser_config.fake_video_file {
            chrome_args.push(format!("--use-file-for-fake-video-capture={}", path));
        }
    }

    let mut config = browser::BrowserConfig::builder();

    if !browser_config.headless {
        config = config.with_head();
    }

    let config = config
        .user_data_dir(&browser_config.user_data_dir)
        .chrome_executable(binary)
        .args(&chrome_args)
        .build()
        .map_err(|e| eyre::eyre!(e))
        .context("failed to build browser config")?;

    browser::Browser::launch(config)
        .await
        .context("failed to launch browser")
}

/// First we attempt to wait for the page to load by waiting for a navigation response.
/// Then we add a loop to check if the element is present in the DOM.
/// In some cases when the processing power is low, the navigation might be completed,
/// but the page is still rendering the elements.
async fn wait_for_element(page: &Page, selector: &str, timeout: Duration) -> Result<Element> {
    let now = std::time::Instant::now();

    loop {
        if let Ok(element) = page.find_element(selector).await {
            return Ok(element);
        }

        // Sleep for a short duration to avoid busy waiting
        tokio::time::sleep(Duration::from_millis(100)).await;

        if now.elapsed() > timeout {
            return Err(eyre::eyre!("timeout waiting for element: {}", selector));
        }
    }
}
