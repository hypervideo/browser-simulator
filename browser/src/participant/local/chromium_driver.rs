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
            self.page
                .find_element(&selector)
                .await
                .with_context(|| format!("Could not find {selector}"))?
                .click()
                .await
                .with_context(|| format!("Could not click {selector}"))?;
            Ok(())
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

    fn set_cookie(&self, domain: &str, name: &str, value: &str) -> BoxFuture<'_, Result<()>> {
        use chromiumoxide::cdp::browser_protocol::network::CookieParam;

        let domain = domain.to_owned();
        let name = name.to_owned();
        let value = value.to_owned();
        async move {
            let cookie = CookieParam::builder()
                .name(name)
                .value(value)
                .domain(domain)
                .path("/")
                .build()
                .map_err(|e| eyre::eyre!("failed to build cookie: {e}"))?;
            self.page
                .set_cookies(vec![cookie])
                .await
                .context("failed to set cookie")?;
            Ok(())
        }
        .boxed()
    }
}
