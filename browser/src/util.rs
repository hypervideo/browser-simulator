use chromiumoxide::{
    Element,
    Page,
};
use eyre::Result;
use std::time::Duration;

/// Poll for a DOM element until it appears or the timeout elapses.
pub(crate) async fn wait_for_element(page: &Page, selector: &str, timeout: Duration) -> Result<Element> {
    let now = std::time::Instant::now();

    loop {
        if let Ok(element) = page.find_element(selector).await {
            return Ok(element);
        }

        // Sleep for a short duration to avoid busy waiting.
        tokio::time::sleep(Duration::from_millis(100)).await;

        if now.elapsed() > timeout {
            return Err(eyre::eyre!("timeout waiting for element: {}", selector));
        }
    }
}
