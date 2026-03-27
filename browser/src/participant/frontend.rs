use super::{
    messages::{
        ParticipantLogMessage,
        ParticipantMessage,
    },
    ParticipantState,
};
use crate::create_browser;
use chromiumoxide::{
    cdp::browser_protocol::target::CreateTargetParams,
    Browser,
    Element,
    Handler,
    Page,
};
use client_simulator_config::{
    BrowserConfig,
    NoiseSuppression,
    ParticipantConfig,
    WebcamResolution,
};
use eyre::{
    bail,
    Context as _,
    ContextCompat as _,
    Result,
};
use futures::{
    future::BoxFuture,
    StreamExt as _,
};
use tokio::{
    sync::{
        mpsc::{
            UnboundedReceiver,
            UnboundedSender,
        },
        watch,
    },
    task::JoinHandle,
};
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ResolvedFrontendKind {
    HyperCore,
    HyperLite,
}

impl ResolvedFrontendKind {
    pub(super) fn from_session_url(session_url: &Url) -> Self {
        let path = session_url.path();
        if path == "/m" || path.starts_with("/m/") {
            Self::HyperLite
        } else {
            Self::HyperCore
        }
    }
}

#[derive(Debug)]
pub(super) struct DriverContext {
    pub(super) participant_config: ParticipantConfig,
    pub(super) page: Page,
    pub(super) state: watch::Sender<ParticipantState>,
    pub(super) sender: UnboundedSender<ParticipantLogMessage>,
}

impl DriverContext {
    pub(super) fn participant_name(&self) -> &str {
        &self.participant_config.username
    }

    pub(super) fn send_log_message(&self, level: &str, message: impl ToString) {
        if let Err(err) = self
            .sender
            .send(ParticipantLogMessage::new(level, self.participant_name(), message))
        {
            trace!("Failed to send log message: {err}");
        }
    }

    pub(super) async fn find_element(&self, selector: &str) -> Result<Element> {
        self.page
            .find_element(selector)
            .await
            .context(format!("Could not find the {selector} element"))
    }
}

pub(super) trait FrontendDriver {
    fn participant_name(&self) -> &str;
    fn state(&self) -> &watch::Sender<ParticipantState>;
    fn log_message(&self, level: &str, message: String);
    fn join(&mut self) -> BoxFuture<'_, Result<()>>;
    fn leave(&self) -> BoxFuture<'_, Result<()>>;
    fn close(self, browser: Browser) -> BoxFuture<'static, Result<()>>
    where
        Self: Sized;
    fn toggle_audio(&self) -> BoxFuture<'_, Result<()>>;
    fn toggle_video(&self) -> BoxFuture<'_, Result<()>>;
    fn toggle_screen_share(&self) -> BoxFuture<'_, Result<()>>;
    fn set_webcam_resolutions(&self, value: WebcamResolution) -> BoxFuture<'_, Result<()>>;
    fn set_noise_suppression(&self, value: NoiseSuppression) -> BoxFuture<'_, Result<()>>;
    fn toggle_background_blur(&self) -> BoxFuture<'_, Result<()>>;
    fn update_state(&self) -> BoxFuture<'_, ()>;
}

pub(super) async fn run_local_participant<D, F>(
    participant_config: ParticipantConfig,
    receiver: UnboundedReceiver<ParticipantMessage>,
    sender: UnboundedSender<ParticipantLogMessage>,
    state: watch::Sender<ParticipantState>,
    make_driver: F,
) -> Result<()>
where
    D: FrontendDriver,
    F: FnOnce(DriverContext) -> D,
{
    let participant_name = participant_config.username.clone();
    let (mut browser, handler) = create_browser(&BrowserConfig::from(&participant_config)).await?;
    let browser_event_task_handle = drive_browser_events(&participant_name, handler);
    let page = create_page_retry(&participant_config, &mut browser).await?;

    state.send_modify(|state| {
        state.username = participant_name.clone();
    });

    let driver = make_driver(DriverContext {
        participant_config,
        page,
        state: state.clone(),
        sender,
    });

    handle_actions(browser, receiver, driver)
        .await
        .context("failed to handle actions")?;

    browser_event_task_handle.await?;

    Ok(())
}

fn drive_browser_events(name: &str, mut handler: Handler) -> JoinHandle<()> {
    let name = name.to_string();
    tokio::task::spawn(async move {
        while let Some(event) = handler.next().await {
            if let Err(err) = event {
                if err.to_string().contains("ResetWithoutClosingHandshake") {
                    error!(participant = %name, "Browser unexpectedly closed");
                    break;
                }
                error!(participant = %name, "error in browser handler: {err:?}");
            }
        }
        debug!(participant = %name, "Browser event handler stopped");
    })
}

async fn handle_actions<D>(
    mut browser: Browser,
    mut receiver: UnboundedReceiver<ParticipantMessage>,
    mut driver: D,
) -> Result<()>
where
    D: FrontendDriver,
{
    driver.state().send_modify(|state| {
        state.running = true;
    });

    if let Err(err) = driver.join().await {
        error!(
            participant = %driver.participant_name(),
            "Failed joining the session when starting the browser {err}"
        );
        driver.log_message(
            "error",
            format!("Failed joining the session when starting the browser {err}"),
        );
        kill_browser(&mut browser, &driver).await;
        driver.state().send_modify(|state| {
            state.running = false;
        });
        return Ok(());
    }

    let mut detached_event = browser
        .event_listener::<chromiumoxide::cdp::browser_protocol::target::EventDetachedFromTarget>()
        .await
        .expect("failed to create event listener");

    loop {
        let message = tokio::select! {
            biased;

            Some(_) = detached_event.next() => {
                warn!(participant = %driver.participant_name(), "Browser unexpectedly closed");
                kill_browser(&mut browser, &driver).await;
                break;
            },

            Some(message) = receiver.recv() => {
                message
            }
        };

        let result = match message {
            ParticipantMessage::Join => driver.join().await,
            ParticipantMessage::Leave => driver.leave().await,
            ParticipantMessage::Close => return driver.close(browser).await,
            ParticipantMessage::ToggleAudio => driver.toggle_audio().await,
            ParticipantMessage::ToggleVideo => driver.toggle_video().await,
            ParticipantMessage::ToggleScreenshare => driver.toggle_screen_share().await,
            ParticipantMessage::SetWebcamResolutions(value) => driver.set_webcam_resolutions(value).await,
            ParticipantMessage::SetNoiseSuppression(value) => driver.set_noise_suppression(value).await,
            ParticipantMessage::ToggleBackgroundBlur => driver.toggle_background_blur().await,
        };

        if let Err(err) = result {
            error!(
                participant = %driver.participant_name(),
                "Running action {message} failed with error: {err}."
            );
            driver.log_message("error", format!("Running action {message} failed with error: {err}."));
        }

        driver.update_state().await;
    }

    driver.state().send_modify(|state| {
        state.running = false;
    });

    Ok(())
}

async fn kill_browser<D>(browser: &mut Browser, driver: &D)
where
    D: FrontendDriver,
{
    match browser.kill().await {
        Some(Ok(_)) => {
            debug!(participant = %driver.participant_name(), "browser killed");
            driver.log_message("debug", "browser killed".to_string());
        }
        Some(Err(err)) => {
            error!(participant = %driver.participant_name(), "failed to kill browser: {err}");
            driver.log_message("error", format!("failed to kill browser: {err}"));
        }
        None => {
            debug!(participant = %driver.participant_name(), "browser process not found");
            driver.log_message("debug", "browser process not found".to_string());
        }
    }
}

async fn create_page(config: &ParticipantConfig, browser: &mut Browser) -> Result<Page> {
    let page = if let Ok(Some(page)) = browser
        .pages()
        .await
        .context("failed to get pages")
        .map(|pages| pages.into_iter().next())
    {
        page.goto(config.session_url.to_string())
            .await
            .context("failed to navigate to session_url")?;
        page
    } else {
        browser
            .new_page(
                CreateTargetParams::builder()
                    .url(config.session_url.to_string())
                    .build()
                    .map_err(|e| eyre::eyre!(e))?,
            )
            .await
            .context("failed to create new page")?
    };

    let navigation = page
        .wait_for_navigation_response()
        .await
        .context("Page could not navigate to session_url")?
        .with_context(|| {
            format!(
                "{}: No request returned when creating a page for {}",
                config.username, config.session_url,
            )
        })?;

    if let Some(text) = &navigation.failure_text {
        bail!(
            "{}: When creating a new page request got a failure: {}",
            config.username,
            text
        );
    }

    debug!(participant = %config.username, "Created a new page for {}", config.session_url);

    Ok(page)
}

async fn create_page_retry(config: &ParticipantConfig, browser: &mut Browser) -> Result<Page> {
    let mut backoff = maybe_backoff::MaybeBackoff::default();
    let mut attempt = 0;
    loop {
        backoff.sleep().await;
        match create_page(config, browser).await {
            Ok(page) => return Ok(page),
            Err(_) if attempt < 5 => {
                attempt += 1;
                backoff.arm();
                warn!(participant = %config.username, ?attempt, "Failed to create a new page, retrying...");
            }
            Err(err) => return Err(err),
        }
    }
}

pub(super) async fn element_state(el: &Element) -> Option<bool> {
    el.attribute("data-test-state")
        .await
        .ok()
        .unwrap_or(None)
        .map(|value| value == "true")
}
