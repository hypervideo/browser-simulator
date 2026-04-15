use crate::{
    auth::{
        BorrowedCookie,
        HyperSessionCookieManger,
    },
    participant::{
        local::{
            core::ParticipantInner,
            frontend::{
                FrontendAutomation,
                FrontendContext,
            },
            lite::ParticipantInnerLite,
        },
        shared::{
            messages::{
                ParticipantLogMessage,
                ParticipantMessage,
            },
            DriverTermination,
            ParticipantDriverSession,
            ParticipantLaunchSpec,
            ResolvedFrontendKind,
        },
    },
};
use chromiumoxide::{
    browser,
    cdp::browser_protocol::target::{
        CreateTargetParams,
        EventDetachedFromTarget,
    },
    Browser,
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
    bail,
    Context as _,
    ContextCompat as _,
    Result,
};
use futures::{
    future::BoxFuture,
    FutureExt as _,
    StreamExt as _,
};
use std::{
    path::PathBuf,
    sync::Arc,
};
use tokio::{
    sync::{
        mpsc::UnboundedSender,
        watch,
    },
    task::JoinHandle,
};

pub(crate) struct LocalChromiumSession {
    launch_spec: ParticipantLaunchSpec,
    browser_config: BrowserConfig,
    sender: UnboundedSender<ParticipantLogMessage>,
    frontend_builder: Option<LocalFrontendBuilder>,
    automation: Option<Box<dyn FrontendAutomation>>,
    browser: Option<Browser>,
    page: Option<Page>,
    browser_event_task: Option<JoinHandle<()>>,
    detached_target_task: Option<JoinHandle<()>>,
    termination_tx: watch::Sender<Option<DriverTermination>>,
    termination_rx: watch::Receiver<Option<DriverTermination>>,
}

enum LocalFrontendBuilder {
    HyperCore {
        auth: Option<BorrowedCookie>,
        cookie_manager: HyperSessionCookieManger,
    },
    HyperLite,
}

impl LocalChromiumSession {
    pub(crate) fn new(
        launch_spec: ParticipantLaunchSpec,
        browser_config: BrowserConfig,
        sender: UnboundedSender<ParticipantLogMessage>,
        auth: Option<BorrowedCookie>,
        cookie_manager: HyperSessionCookieManger,
    ) -> Self {
        let frontend_builder = match launch_spec.frontend_kind {
            ResolvedFrontendKind::HyperCore => LocalFrontendBuilder::HyperCore { auth, cookie_manager },
            ResolvedFrontendKind::HyperLite => LocalFrontendBuilder::HyperLite,
        };
        let (termination_tx, termination_rx) = watch::channel(None);

        Self {
            launch_spec,
            browser_config,
            sender,
            frontend_builder: Some(frontend_builder),
            automation: None,
            browser: None,
            page: None,
            browser_event_task: None,
            detached_target_task: None,
            termination_tx,
            termination_rx,
        }
    }

    fn log_message(&self, level: &str, message: impl ToString) {
        let log_message = ParticipantLogMessage::new(level, &self.launch_spec.username, message);
        log_message.write();
        if let Err(err) = self.sender.send(log_message) {
            trace!(
                participant = %self.launch_spec.username,
                "Failed to send local driver log message: {err}"
            );
        }
    }

    async fn start_inner(&mut self) -> Result<()> {
        let (mut browser, handler) = create_browser(&self.browser_config).await?;
        let browser_event_task = drive_browser_events(&self.launch_spec.username, handler, self.termination_tx.clone());
        let page = create_page_retry(&self.launch_spec, &mut browser).await?;
        let detached_target_task =
            drive_detached_target_events(&self.launch_spec.username, &mut browser, self.termination_tx.clone()).await?;

        let frontend_builder = self
            .frontend_builder
            .take()
            .context("local frontend builder already consumed")?;
        let automation = frontend_builder
            .build(FrontendContext {
                launch_spec: self.launch_spec.clone(),
                page: page.clone(),
                sender: self.sender.clone(),
            })
            .await?;

        self.browser = Some(browser);
        self.page = Some(page);
        self.browser_event_task = Some(browser_event_task);
        self.detached_target_task = Some(detached_target_task);
        self.automation = Some(automation);

        if let Err(err) = self.automation_mut()?.join().await {
            self.kill_browser().await;
            return Err(err);
        }

        Ok(())
    }

    fn automation_mut(&mut self) -> Result<&mut (dyn FrontendAutomation + 'static)> {
        self.automation
            .as_deref_mut()
            .context("local frontend automation not started")
    }

    async fn close_inner(&mut self) -> Result<()> {
        if let Some(handle) = self.detached_target_task.take() {
            handle.abort();
        }

        if let Some(automation) = self.automation.as_mut() {
            if let Err(err) = automation.leave().await {
                self.log_message("error", format!("Failed leaving space while closing browser: {err}"));
            }
        }

        if let Some(page) = self.page.take() {
            if let Err(err) = page.close().await {
                self.log_message("error", format!("Error closing page: {err}"));
            }
        }

        if let Some(browser) = self.browser.as_mut() {
            browser.close().await?;
            browser.wait().await?;
        }

        if let Some(handle) = self.browser_event_task.take() {
            let _ = handle.await;
        }

        self.browser = None;
        self.automation = None;

        self.log_message("info", "Closed the browser");

        Ok(())
    }

    async fn kill_browser(&mut self) {
        if let Some(handle) = self.detached_target_task.take() {
            handle.abort();
        }

        if let Some(browser) = self.browser.as_mut() {
            match browser.kill().await {
                Some(Ok(_)) => self.log_message("debug", "browser killed"),
                Some(Err(err)) => self.log_message("error", format!("failed to kill browser: {err}")),
                None => self.log_message("debug", "browser process not found"),
            }
        }

        if let Some(handle) = self.browser_event_task.take() {
            let _ = handle.await;
        }

        self.browser = None;
        self.page = None;
        self.automation = None;
    }

    async fn wait_for_termination_inner(&mut self) -> DriverTermination {
        loop {
            if let Some(termination) = self.termination_rx.borrow().clone() {
                return termination;
            }

            if self.termination_rx.changed().await.is_err() {
                return DriverTermination::new("warn", "local driver termination channel closed");
            }
        }
    }
}

impl ParticipantDriverSession for LocalChromiumSession {
    fn participant_name(&self) -> &str {
        &self.launch_spec.username
    }

    fn start(&mut self) -> BoxFuture<'_, Result<()>> {
        async move { self.start_inner().await }.boxed()
    }

    fn handle_command(&mut self, message: ParticipantMessage) -> BoxFuture<'_, Result<()>> {
        async move { self.automation_mut()?.handle_command(message).await }.boxed()
    }

    fn refresh_state(&mut self) -> BoxFuture<'_, Result<crate::participant::shared::ParticipantState>> {
        async move { self.automation_mut()?.refresh_state().await }.boxed()
    }

    fn close(&mut self) -> BoxFuture<'_, Result<()>> {
        async move { self.close_inner().await }.boxed()
    }

    fn wait_for_termination(&mut self) -> BoxFuture<'_, DriverTermination> {
        async move { self.wait_for_termination_inner().await }.boxed()
    }
}

impl LocalFrontendBuilder {
    async fn build(self, context: FrontendContext) -> Result<Box<dyn FrontendAutomation>> {
        match self {
            Self::HyperCore { auth, cookie_manager } => {
                let auth = if let Some(cookie) = auth {
                    cookie
                } else {
                    cookie_manager
                        .fetch_new_cookie(context.launch_spec.base_url(), context.participant_name())
                        .await?
                };
                Ok(Box::new(ParticipantInner::new(context, auth)))
            }
            Self::HyperLite => Ok(Box::new(ParticipantInnerLite::new(context))),
        }
    }
}

fn get_binary() -> Result<PathBuf> {
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

async fn create_browser(browser_config: &BrowserConfig) -> Result<(Browser, Handler)> {
    let binary = get_binary()?;

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
        config = config.with_head().window_size(1920, 1080).viewport(None);
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

fn drive_browser_events(
    name: &str,
    mut handler: Handler,
    termination_tx: watch::Sender<Option<DriverTermination>>,
) -> JoinHandle<()> {
    let participant_name = name.to_string();
    tokio::spawn(async move {
        while let Some(event) = handler.next().await {
            if let Err(err) = event {
                if err.to_string().contains("ResetWithoutClosingHandshake") {
                    error!(participant = %participant_name, "Browser unexpectedly closed");
                    signal_termination(
                        &termination_tx,
                        DriverTermination::new("warn", "Browser unexpectedly closed"),
                    );
                    break;
                }

                error!(participant = %participant_name, "error in browser handler: {err:?}");
            }
        }

        debug!(participant = %participant_name, "Browser event handler stopped");
    })
}

async fn drive_detached_target_events(
    name: &str,
    browser: &mut Browser,
    termination_tx: watch::Sender<Option<DriverTermination>>,
) -> Result<JoinHandle<()>> {
    let participant_name = Arc::new(name.to_string());
    let mut detached_event = browser
        .event_listener::<EventDetachedFromTarget>()
        .await
        .context("failed to create detached target event listener")?;

    Ok(tokio::spawn(async move {
        if detached_event.next().await.is_some() {
            warn!(participant = %participant_name, "Browser unexpectedly closed");
            signal_termination(
                &termination_tx,
                DriverTermination::new("warn", "Browser unexpectedly closed"),
            );
        }
    }))
}

fn signal_termination(termination_tx: &watch::Sender<Option<DriverTermination>>, termination: DriverTermination) {
    if termination_tx.borrow().is_none() {
        let _ = termination_tx.send(Some(termination));
    }
}

async fn create_page(launch_spec: &ParticipantLaunchSpec, browser: &mut Browser) -> Result<Page> {
    let page = if let Ok(Some(page)) = browser
        .pages()
        .await
        .context("failed to get pages")
        .map(|pages| pages.into_iter().next())
    {
        page.goto(launch_spec.session_url.to_string())
            .await
            .context("failed to navigate to session_url")?;
        page
    } else {
        browser
            .new_page(
                CreateTargetParams::builder()
                    .url(launch_spec.session_url.to_string())
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
                launch_spec.username, launch_spec.session_url,
            )
        })?;

    if let Some(text) = &navigation.failure_text {
        bail!(
            "{}: When creating a new page request got a failure: {}",
            launch_spec.username,
            text
        );
    }

    debug!(
        participant = %launch_spec.username,
        "Created a new page for {}",
        launch_spec.session_url
    );

    Ok(page)
}

async fn create_page_retry(launch_spec: &ParticipantLaunchSpec, browser: &mut Browser) -> Result<Page> {
    let mut backoff = maybe_backoff::MaybeBackoff::default();
    let mut attempt = 0;
    loop {
        backoff.sleep().await;
        match create_page(launch_spec, browser).await {
            Ok(page) => return Ok(page),
            Err(_) if attempt < 5 => {
                attempt += 1;
                backoff.arm();
                warn!(
                    participant = %launch_spec.username,
                    ?attempt,
                    "Failed to create a new page, retrying..."
                );
            }
            Err(err) => return Err(err),
        }
    }
}
