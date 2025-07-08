#[macro_use]
extern crate tracing;

use chromiumoxide::{
    browser,
    Browser,
    Element,
    Handler,
    Page,
};
use client_simulator_config::{
    media::{
        FakeMedia,
        FakeMediaFiles,
    },
    BrowserConfig,
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
    match &browser_config.app_config.fake_media() {
        FakeMedia::None => {}
        FakeMedia::Builtin => {
            chrome_args.extend([
                "--no-sandbox".to_string(),
                "--use-fake-ui-for-media-stream".to_string(),
                "--use-fake-device-for-media-stream".to_string(),
            ]);
        }
        FakeMedia::FileOrUrl(file_or_url) => {
            chrome_args.extend([
                "--no-sandbox".to_string(),
                "--use-fake-ui-for-media-stream".to_string(),
                "--use-fake-device-for-media-stream".to_string(),
            ]);

            let fake_media = tokio::task::block_in_place(move || {
                match file_or_url
                    .parse()
                    .and_then(|input| FakeMediaFiles::from_file_or_url(input, &browser_config.cache_dir))
                {
                    Ok(media) => Some(media),
                    Err(err) => {
                        error!("Unable to read custom fake media from {file_or_url:?}: {err}");
                        None
                    }
                }
            });

            if let Some(media) = fake_media {
                if let Some(audio) = media.audio {
                    chrome_args.push(format!("--use-file-for-fake-audio-capture={}", audio.display()));
                }
                if let Some(video) = media.video {
                    chrome_args.push(format!("--use-file-for-fake-video-capture={}", video.display()));
                }
            }
        }
    }

    let mut config = browser::BrowserConfig::builder();

    if !browser_config.app_config.headless {
        config = config.with_head().window_size(1920, 1080).viewport(None) // Fill the entire window
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
