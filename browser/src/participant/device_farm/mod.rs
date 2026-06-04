mod control;
mod test_grid;
mod webdriver_driver;

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
pub use aws_sdk_devicefarm::types::TestGridSessionStatus;
use client_simulator_config::{
    media::FakeMedia,
    DeviceFarmConfig,
};
pub use control::{
    close_test_grid_session,
    list_active_project_sessions,
    list_project_sessions,
    DeviceFarmCloseResult,
    DeviceFarmSessionInfo,
};
use eyre::{
    bail,
    Context as _,
    ContextCompat as _,
    Report,
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
pub use test_grid::{
    AwsTestGrid,
    TestGridApi,
};
use thirtyfour::{
    common::config::WebDriverConfig,
    prelude::{
        WebDriverError,
        WebDriverResult,
    },
    session::http::{
        Body,
        HttpClient,
    },
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
#[allow(unused_imports)]
pub(crate) use webdriver_driver::WebDriverDriver;

const DEVICE_FARM_MAX_DURATION_MIN_SECS: u64 = 180;
const DEVICE_FARM_MAX_DURATION_MAX_SECS: u64 = 2400;
const DEVICE_FARM_IDLE_TIMEOUT_MIN_SECS: u64 = 30;
const DEVICE_FARM_IDLE_TIMEOUT_MAX_SECS: u64 = 900;

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
    webdriver: Option<WebDriver>,
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
            webdriver: None,
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
            serde_json::json!(aws_duration_secs(
                config.session_max_duration_ms,
                DEVICE_FARM_MAX_DURATION_MIN_SECS,
                DEVICE_FARM_MAX_DURATION_MAX_SECS,
            )),
        );
        caps.insert_base_capability(
            "aws:idleTimeoutSecs".to_string(),
            serde_json::json!(aws_duration_secs(
                config.idle_timeout_ms,
                DEVICE_FARM_IDLE_TIMEOUT_MIN_SECS,
                DEVICE_FARM_IDLE_TIMEOUT_MAX_SECS,
            )),
        );
        Ok(caps)
    }

    async fn connect(api: Arc<dyn TestGridApi>, config: DeviceFarmConfig) -> Result<WebDriver> {
        let url = api
            .create_test_grid_url(&config.project_arn, config.url_expires_seconds)
            .await?;
        let caps = Self::build_capabilities(&config)?;
        let endpoint_url: url::Url = url
            .parse()
            .context("Device Farm returned an invalid Selenium endpoint URL")?;
        debug!(
            endpoint_host = endpoint_url.host_str().unwrap_or("<unknown>"),
            endpoint_uses_wd_hub = endpoint_url.path().ends_with("/wd/hub"),
            "Received Device Farm Selenium endpoint",
        );
        let client = DeviceFarmHttpClient::new(endpoint_url);
        // Device Farm can take 60-120s before a requested session becomes usable.
        WebDriver::new_with_config_and_client(&url, caps, WebDriverConfig::default(), client)
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
        self.webdriver = Some(driver.clone());
        let webdriver_driver = WebDriverDriver::new(driver);

        let auth = self.auth.take().context("device farm auth already consumed")?;
        let context = FrontendContext {
            launch_spec: self.launch_spec.clone(),
            driver: Box::new(webdriver_driver),
            sender: self.sender.clone(),
        };
        let mut automation = FrontendKindBuilder::build(context, auth).await?;

        self.termination_tx.send_replace(None);
        self.start_max_duration_poller();
        if let Err(err) = automation.join().await {
            self.stop_max_duration_poller().await;
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
        let budget = aws_duration_secs(
            self.config.idle_timeout_ms,
            DEVICE_FARM_IDLE_TIMEOUT_MIN_SECS,
            DEVICE_FARM_IDLE_TIMEOUT_MAX_SECS,
        )
        .saturating_mul(500)
        .max(1);
        Duration::from_millis(configured.min(budget))
    }

    fn effective_max_duration(&self) -> Duration {
        Duration::from_secs(aws_duration_secs(
            self.config.session_max_duration_ms,
            DEVICE_FARM_MAX_DURATION_MIN_SECS,
            DEVICE_FARM_MAX_DURATION_MAX_SECS,
        ))
    }

    fn start_max_duration_poller(&mut self) {
        // Periodic runtime state refreshes send WebDriver commands and keep the
        // AWS idle timeout alive. This task only mirrors the AWS hard session cap.
        let interval = self.effective_poll_interval();
        let max_duration = self.effective_max_duration();
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

    async fn stop_max_duration_poller(&mut self) {
        if let Some(tx) = self.poller_shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(task) = self.poller_task.take() {
            let _ = task.await;
        }
    }

    async fn close_inner(&mut self) -> Result<()> {
        self.stop_max_duration_poller().await;

        if let Some(mut automation) = self.automation.take() {
            let joined = automation
                .refresh_state()
                .await
                .map(|state| state.joined)
                .unwrap_or(false);
            if joined {
                if let Err(err) = automation.leave().await {
                    self.log_message("error", format!("Failed leaving space while closing: {err}"));
                }
            }
        }

        if let Some(driver) = self.webdriver.take() {
            if let Err(err) = driver.quit().await {
                self.log_message("error", format!("Failed closing WebDriver session: {err}"));
            }
        }

        self.cached_state.running = false;
        self.cached_state.joined = false;
        self.cached_state.screenshare_activated = false;
        self.log_message("info", "Closed Device Farm browser session");
        Ok(())
    }

    async fn ping_webdriver(driver: WebDriver) -> Result<()> {
        driver
            .current_url()
            .await
            .context("Device Farm WebDriver session is not responsive")?;
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

#[derive(Clone)]
struct DeviceFarmHttpClient {
    signed_url: url::Url,
    inner: reqwest::Client,
}

impl DeviceFarmHttpClient {
    fn new(signed_url: url::Url) -> Self {
        Self {
            signed_url,
            inner: reqwest::Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl HttpClient for DeviceFarmHttpClient {
    async fn send(&self, request: http::Request<Body<'_>>) -> WebDriverResult<http::Response<bytes::Bytes>> {
        let request = rewrite_device_farm_request_uri(&self.signed_url, request)?;
        HttpClient::send(&self.inner, request).await
    }

    async fn new(&self) -> Arc<dyn HttpClient> {
        Arc::new(Self::new(self.signed_url.clone()))
    }
}

fn rewrite_device_farm_request_uri<'a>(
    signed_url: &url::Url,
    request: http::Request<Body<'a>>,
) -> WebDriverResult<http::Request<Body<'a>>> {
    let (mut parts, body) = request.into_parts();
    let rewritten = signed_test_grid_command_url(signed_url, &parts.uri)?;
    parts.uri = rewritten
        .as_str()
        .parse()
        .map_err(|err| WebDriverError::ParseError(format!("invalid Device Farm WebDriver request URI: {err}")))?;
    Ok(http::Request::from_parts(parts, body))
}

fn signed_test_grid_command_url(signed_url: &url::Url, command_uri: &http::Uri) -> WebDriverResult<url::Url> {
    control::signed_test_grid_command_url(signed_url, command_uri)
        .map_err(|err| WebDriverError::ParseError(format!("invalid Device Farm WebDriver request URI: {err}")))
}

fn aws_duration_secs(duration_ms: u64, min_secs: u64, max_secs: u64) -> u64 {
    duration_ms.div_ceil(1000).clamp(min_secs, max_secs)
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
            if self.automation.is_none() {
                return Ok(self.cached_state.clone());
            }

            let driver = self
                .webdriver
                .as_ref()
                .context("Device Farm WebDriver session not started")?
                .clone();
            Self::ping_webdriver(driver).await?;
            let automation = self.automation.as_mut().context("Device Farm automation not started")?;
            let state = automation.refresh_state().await?;
            self.cached_state = state.clone();
            Ok(state)
        }
        .boxed()
    }

    fn close(&mut self) -> BoxFuture<'_, Result<()>> {
        self.close_inner().boxed()
    }

    fn wait_for_termination(&mut self) -> BoxFuture<'_, DriverTermination> {
        self.wait_for_termination_inner().boxed()
    }

    fn state_refresh_interval(&self) -> Option<Duration> {
        Some(self.effective_poll_interval())
    }

    fn state_refresh_error_termination(&self, err: &Report) -> Option<DriverTermination> {
        Some(DriverTermination::new(
            "warn",
            format!("Device Farm browser session stopped responding while refreshing state: {err}"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn capabilities_clamp_aws_duration_ranges() {
        let high_max_low_idle = capabilities_for_duration(3_600_000, 5_000);
        assert_eq!(high_max_low_idle._get("aws:maxDurationSecs"), Some(&json!(2400)));
        assert_eq!(high_max_low_idle._get("aws:idleTimeoutSecs"), Some(&json!(30)));

        let low_max_high_idle = capabilities_for_duration(60_000, 2_000_000);
        assert_eq!(low_max_high_idle._get("aws:maxDurationSecs"), Some(&json!(180)));
        assert_eq!(low_max_high_idle._get("aws:idleTimeoutSecs"), Some(&json!(900)));
    }

    fn capabilities_for_duration(session_max_duration_ms: u64, idle_timeout_ms: u64) -> ChromeCapabilities {
        let config = DeviceFarmConfig {
            session_max_duration_ms,
            idle_timeout_ms,
            ..DeviceFarmConfig::default()
        };
        DeviceFarmSession::build_capabilities(&config).unwrap()
    }
}
