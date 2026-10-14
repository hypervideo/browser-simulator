use crate::participant::frontend::BrowserDriver;
use chromiumoxide::{
    cdp::js_protocol::runtime::{
        CallArgument,
        CallFunctionOnParams,
    },
    js::Evaluation,
    Page,
};
use eyre::{
    Context as _,
    Result,
};
use futures::{
    future::BoxFuture,
    FutureExt as _,
};
use std::time::{
    Duration,
    Instant,
};

const CLICK_ATTEMPTS: usize = 3;

/// `BrowserDriver` implementation backed by a local chromiumoxide `Page` (CDP).
pub(crate) struct ChromiumDriver {
    page: Page,
}

impl ChromiumDriver {
    pub(crate) fn new(page: Page) -> Self {
        Self { page }
    }
}

impl BrowserDriver for ChromiumDriver {
    fn goto(&self, url: &str) -> BoxFuture<'_, Result<()>> {
        let url = url.to_owned();
        async move {
            self.page.goto(url).await.context("failed to navigate")?;
            Ok(())
        }
        .boxed()
    }

    fn exists(&self, selector: &str) -> BoxFuture<'_, Result<bool>> {
        let selector = selector.to_owned();
        async move { Ok(self.page.find_element(selector).await.is_ok()) }.boxed()
    }

    fn wait_for(&self, selector: &str, timeout: Duration) -> BoxFuture<'_, Result<()>> {
        let selector = selector.to_owned();
        async move {
            let start = Instant::now();
            loop {
                if self.page.find_element(&selector).await.is_ok() {
                    return Ok(());
                }
                if start.elapsed() > timeout {
                    eyre::bail!("timeout waiting for selector {selector}");
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        .boxed()
    }

    fn click(&self, selector: &str) -> BoxFuture<'_, Result<()>> {
        let selector = selector.to_owned();
        async move {
            let element = self
                .page
                .find_element(&selector)
                .await
                .with_context(|| format!("Could not find {selector}"))?;
            // chromiumoxide reads the element position before it presses the mouse. When the
            // layout shifts in between, the press hits another element, so press again.
            for _ in 0..CLICK_ATTEMPTS {
                element
                    .call_js_fn(
                        "function() { this.simulatorPressed = false; this.addEventListener('pointerdown', () => { this.simulatorPressed = true; }, { capture: true, once: true }); }",
                        false,
                    )
                    .await
                    .context("failed to watch for the mouse press")?;
                element
                    .click()
                    .await
                    .with_context(|| format!("Could not click {selector}"))?;
                // A click that navigates destroys the page context. Only a readable `false` is a miss.
                let pressed = element
                    .call_js_fn("function() { return this.simulatorPressed; }", false)
                    .await
                    .ok()
                    .and_then(|result| result.result.value);
                if pressed != Some(serde_json::Value::Bool(false)) {
                    return Ok(());
                }
            }
            eyre::bail!("Could not click {selector}: the mouse press kept hitting another element")
        }
        .boxed()
    }

    fn fill(&self, selector: &str, text: &str) -> BoxFuture<'_, Result<()>> {
        let selector = selector.to_owned();
        let text = text.to_owned();
        async move {
            let element = self
                .page
                .find_element(&selector)
                .await
                .with_context(|| format!("Could not find {selector}"))?;
            element
                .focus()
                .await
                .context("failed to focus element")?
                .call_js_fn("function() { this.value = ''; }", true)
                .await
                .context("failed to clear element")?;
            element.type_str(&text).await.context("failed to type into element")?;
            Ok(())
        }
        .boxed()
    }

    fn attribute(&self, selector: &str, name: &str) -> BoxFuture<'_, Result<Option<String>>> {
        let selector = selector.to_owned();
        let name = name.to_owned();
        async move {
            match self.page.find_element(&selector).await {
                Ok(element) => Ok(element.attribute(&name).await.ok().flatten()),
                Err(_) => Ok(None),
            }
        }
        .boxed()
    }

    fn eval(&self, js_body: &str, arg: Option<serde_json::Value>) -> BoxFuture<'_, Result<serde_json::Value>> {
        let declaration = format!("function() {{ {js_body} }}");
        async move {
            let mut builder = CallFunctionOnParams::builder().function_declaration(declaration);
            if let Some(arg) = arg {
                let argument = CallArgument::builder().value(arg).build();
                builder = builder.arguments(vec![argument]);
            }
            let function = builder
                .build()
                .map_err(|e| eyre::eyre!("failed to build eval command: {e}"))?;
            let result = self
                .page
                .evaluate(Evaluation::Function(function))
                .await
                .context("failed to evaluate script")?;
            Ok(result.into_value().unwrap_or(serde_json::Value::Null))
        }
        .boxed()
    }

    fn inject_first_party_credentials(&self, realm: &str, credentials: &str) -> BoxFuture<'_, Result<()>> {
        let script = super::super::frontend::first_party_credentials_init_script(realm, credentials);
        async move {
            self.page
                .add_init_script(script?)
                .await
                .context("failed to install first-party credential init script")?;
            Ok(())
        }
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use client_simulator_config::{
        BrowserConfig,
        Config,
    };
    use futures::StreamExt as _;

    // Requires Chromium/Chrome; the Nix devshell provides Chromium on Linux.
    #[tokio::test]
    async fn click_reaches_an_element_that_moves_before_the_mouse_press() {
        let config = BrowserConfig {
            app_config: Config {
                headless: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let (mut browser, mut handler) = super::super::session::create_browser(&config).await.unwrap();
        let events = tokio::spawn(async move { while handler.next().await.is_some() {} });
        let page = browser.new_page("about:blank").await.unwrap();
        // Model the Hyper lobby: content appears above the join button after the
        // driver has read the button position but before it presses the mouse.
        page.set_content(
            r#"<div id="spacer"></div>
            <button onclick="window.clicks = (window.clicks ?? 0) + 1">Join</button>
            <script>
                document.addEventListener('mousemove', () => { spacer.style.height = '100px'; }, { once: true });
            </script>"#,
        )
        .await
        .unwrap();
        let driver = ChromiumDriver::new(page);

        let clicked = driver.click("button").await;
        let clicks = driver.eval("return window.clicks ?? 0;", None).await;
        browser.close().await.unwrap();
        browser.wait().await.unwrap();
        events.await.unwrap();

        clicked.unwrap();
        assert_eq!(clicks.unwrap(), serde_json::json!(1));
    }
}
