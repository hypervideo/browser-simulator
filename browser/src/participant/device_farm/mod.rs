mod test_grid;
mod webdriver_driver;

#[allow(unused_imports)]
pub(crate) use test_grid::{
    AwsTestGrid,
    TestGridApi,
};
#[allow(unused_imports)]
pub(crate) use webdriver_driver::WebDriverDriver;

use crate::{
    auth::{
        BorrowedCookie,
        HyperSessionCookieManger,
    },
    participant::{
        frontend::{
            FrontendAuth,
            FrontendAutomation,
            FrontendContext,
            FrontendKindBuilder,
        },
        shared::{
            messages::{
                ParticipantLogMessage,
                ParticipantMessage,
            },
            DriverTermination,
            ParticipantDriverSession,
            ParticipantLaunchSpec,
            ParticipantState,
        },
    },
};
use client_simulator_config::{
    media::FakeMedia,
    DeviceFarmConfig,
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
};
use std::{
    sync::Arc,
    time::Duration,
};
use thirtyfour::{
    CapabilitiesHelper,
    ChromeCapabilities,
    ChromiumLikeCapabilities,
    DesiredCapabilities,
    WebDriver,
};
use tokio::{
    sync::{
        mpsc::UnboundedSender,
        oneshot,
        watch,
    },
    task::JoinHandle,
    time::MissedTickBehavior,
};

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub(super) struct DeviceFarmLaunchOptions {
    headless: bool,
    fake_media: FakeMedia,
}

impl From<&client_simulator_config::Config> for DeviceFarmLaunchOptions {
    fn from(config: &client_simulator_config::Config) -> Self {
        Self {
            headless: config.headless,
            fake_media: config.fake_media(),
        }
    }
}

#[allow(dead_code)]
pub(super) struct DeviceFarmSession {
    launch_spec: ParticipantLaunchSpec,
    launch_options: DeviceFarmLaunchOptions,
    config: DeviceFarmConfig,
    sender: UnboundedSender<ParticipantLogMessage>,
    api: Arc<dyn TestGridApi>,
    auth: Option<FrontendAuth>,
    automation: Option<Box<dyn FrontendAutomation>>,
    cached_state: ParticipantState,
    termination_tx: watch::Sender<Option<DriverTermination>>,
    termination_rx: watch::Receiver<Option<DriverTermination>>,
    poller_shutdown_tx: Option<oneshot::Sender<()>>,
    poller_task: Option<JoinHandle<()>>,
}

#[allow(dead_code)]
impl DeviceFarmSession {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        launch_spec: ParticipantLaunchSpec,
        launch_options: DeviceFarmLaunchOptions,
        config: DeviceFarmConfig,
        sender: UnboundedSender<ParticipantLogMessage>,
        cookie: Option<BorrowedCookie>,
        cookie_manager: HyperSessionCookieManger,
        api: Arc<dyn TestGridApi>,
    ) -> Self {
        let auth = FrontendAuth::for_kind(launch_spec.frontend_kind, cookie, cookie_manager);
        let (termination_tx, termination_rx) = watch::channel(None);
        Self {
            cached_state: ParticipantState {
                username: launch_spec.username.clone(),
                ..Default::default()
            },
            launch_spec,
            launch_options,
            config,
            sender,
            api,
            auth: Some(auth),
            automation: None,
            termination_tx,
            termination_rx,
            poller_shutdown_tx: None,
            poller_task: None,
        }
    }

    fn log_message(&self, level: &str, message: impl ToString) {
        let log_message = ParticipantLogMessage::new(level, &self.launch_spec.username, message);
        log_message.write();
        if let Err(err) = self.sender.send(log_message) {
            trace!(participant = %self.launch_spec.username, "Failed to send device farm log message: {err}");
        }
    }

    fn log_backend_limitations(&self) {
        if matches!(self.launch_options.fake_media, FakeMedia::FileOrUrl(_)) {
            self.log_message(
                "warn",
                "Device Farm backend cannot use a local fake-media file/URL; using the synthetic fake device instead",
            );
        }
    }

    fn build_capabilities(config: &DeviceFarmConfig) -> Result<ChromeCapabilities> {
        let mut caps = DesiredCapabilities::chrome();
        // Synthetic fake media only. Device Farm has no access to local fake-media files.
        caps.add_arg("--use-fake-ui-for-media-stream")?;
        caps.add_arg("--use-fake-device-for-media-stream")?;
        caps.insert_base_capability(
            "aws:maxDurationSecs".to_string(),
            serde_json::json!((config.session_max_duration_ms / 1000).max(1)),
        );
        caps.insert_base_capability(
            "aws:idleTimeoutSecs".to_string(),
            serde_json::json!((config.idle_timeout_ms / 1000).max(1)),
        );
        Ok(caps)
    }

    async fn connect(api: Arc<dyn TestGridApi>, config: DeviceFarmConfig) -> Result<WebDriver> {
        let url = api
            .create_test_grid_url(&config.project_arn, config.url_expires_seconds)
            .await?;
        let caps = Self::build_capabilities(&config)?;
        // Device Farm can take 60-120s before a requested session becomes usable.
        WebDriver::new(&url, caps)
            .await
            .context("failed to connect to Device Farm Selenium endpoint")
    }

    async fn start_inner(&mut self) -> Result<()> {
        if self.automation.is_some() {
            bail!("Device Farm session already started");
        }
        self.log_backend_limitations();

        self.log_message(
            "info",
            format!(
                "Requesting Device Farm test grid URL for project {}",
                self.config.project_arn
            ),
        );
        let driver = Self::connect(Arc::clone(&self.api), self.config.clone()).await?;
        let webdriver_driver = WebDriverDriver::new(driver);

        let auth = self.auth.take().context("device farm auth already consumed")?;
        let context = FrontendContext {
            launch_spec: self.launch_spec.clone(),
            driver: Box::new(webdriver_driver),
            sender: self.sender.clone(),
        };
        let mut automation = FrontendKindBuilder::build(context, auth).await?;

        self.start_keep_alive_poller();
        if let Err(err) = automation.join().await {
            self.stop_keep_alive_poller().await;
            return Err(err);
        }

        self.automation = Some(automation);
        self.cached_state.running = true;
        self.log_message("info", "Connected Device Farm browser session");
        Ok(())
    }

    fn automation_mut(&mut self) -> Result<&mut (dyn FrontendAutomation + 'static)> {
        self.automation
            .as_deref_mut()
            .context("Device Farm automation not started")
    }

    fn effective_poll_interval(&self) -> Duration {
        let configured = self.config.health_poll_interval_ms.max(1);
        let budget = self.config.idle_timeout_ms.saturating_div(2).max(1);
        Duration::from_millis(configured.min(budget))
    }

    fn start_keep_alive_poller(&mut self) {
        // The WebDriver is owned by frontend automation, so Phase 4 keeps
        // reclamation implicit: Device Farm reclaims an abandoned session after
        // aws:idleTimeoutSecs, and hard-stops it at aws:maxDurationSecs.
        let interval = self.effective_poll_interval();
        let max_duration = Duration::from_millis(self.config.session_max_duration_ms);
        let termination_tx = self.termination_tx.clone();
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

        let task = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
            ticker.tick().await;
            let started = tokio::time::Instant::now();
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => break,
                    _ = ticker.tick() => {
                        if started.elapsed() >= max_duration {
                            let _ = termination_tx.send(Some(DriverTermination::new(
                                "warn",
                                "Device Farm session reached its configured max duration",
                            )));
                            break;
                        }
                    }
                }
            }
        });

        self.poller_shutdown_tx = Some(shutdown_tx);
        self.poller_task = Some(task);
    }

    async fn stop_keep_alive_poller(&mut self) {
        if let Some(tx) = self.poller_shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(task) = self.poller_task.take() {
            let _ = task.await;
        }
    }

    async fn close_inner(&mut self) -> Result<()> {
        self.stop_keep_alive_poller().await;

        if let Some(mut automation) = self.automation.take() {
            let joined = automation.refresh_state().await.map(|state| state.joined).unwrap_or(false);
            if joined {
                if let Err(err) = automation.leave().await {
                    self.log_message("error", format!("Failed leaving space while closing: {err}"));
                }
            }
        }

        self.cached_state.running = false;
        self.cached_state.joined = false;
        self.cached_state.screenshare_activated = false;
        self.log_message("info", "Closed Device Farm browser session");
        Ok(())
    }

    async fn wait_for_termination_inner(&mut self) -> DriverTermination {
        loop {
            if let Some(termination) = self.termination_rx.borrow().clone() {
                return termination;
            }
            if self.termination_rx.changed().await.is_err() {
                return DriverTermination::new("warn", "device farm termination channel closed");
            }
        }
    }
}

impl ParticipantDriverSession for DeviceFarmSession {
    fn participant_name(&self) -> &str {
        &self.launch_spec.username
    }

    fn start(&mut self) -> BoxFuture<'_, Result<()>> {
        self.start_inner().boxed()
    }

    fn handle_command(&mut self, message: ParticipantMessage) -> BoxFuture<'_, Result<()>> {
        async move { self.automation_mut()?.handle_command(message).await }.boxed()
    }

    fn refresh_state(&mut self) -> BoxFuture<'_, Result<ParticipantState>> {
        async move {
            match self.automation.as_mut() {
                Some(automation) => {
                    let state = automation.refresh_state().await?;
                    self.cached_state = state.clone();
                    Ok(state)
                }
                None => Ok(self.cached_state.clone()),
            }
        }
        .boxed()
    }

    fn close(&mut self) -> BoxFuture<'_, Result<()>> {
        self.close_inner().boxed()
    }

    fn wait_for_termination(&mut self) -> BoxFuture<'_, DriverTermination> {
        self.wait_for_termination_inner().boxed()
    }
}
