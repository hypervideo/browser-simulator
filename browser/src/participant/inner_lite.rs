//! Browser interaction for the hyper-lite ("hyper lite") frontend.

use super::frontend::{
    element_state,
    run_local_participant,
    DriverContext,
    FrontendDriver,
};
use crate::{
    participant::selectors::lite,
    wait_for_element,
};
use chromiumoxide::{
    Browser,
    Element,
};
use client_simulator_config::{
    Config,
    NoiseSuppression,
    ParticipantConfig,
    TransportMode,
    WebcamResolution,
};
use eyre::{
    Context as _,
    Result,
};
use futures::{
    future::BoxFuture,
    FutureExt as _,
};
use std::time::Duration;
use tokio::sync::{
    mpsc::{
        UnboundedReceiver,
        UnboundedSender,
    },
    watch,
};

/// Frontend driver for the hyper-lite UI.
#[derive(Debug)]
pub(super) struct ParticipantInnerLite {
    context: DriverContext,
}

impl ParticipantInnerLite {
    #[instrument(level = "debug", skip_all, fields(name = %participant_config.username))]
    pub(super) async fn run(
        participant_config: ParticipantConfig,
        receiver: UnboundedReceiver<super::messages::ParticipantMessage>,
        sender: UnboundedSender<super::messages::ParticipantLogMessage>,
        state: watch::Sender<super::ParticipantState>,
    ) -> Result<()> {
        debug!(participant = %participant_config.username, "Starting participant inner lite...");
        run_local_participant(participant_config, receiver, sender, state, |context| Self { context }).await
    }

    async fn join_session(&mut self) -> Result<()> {
        if self.context.state.borrow().joined {
            warn!(participant = %self.participant_name(), "Already joined.");
            self.context.send_log_message("warn", "Already joined.");
            return Ok(());
        }

        self.context
            .page
            .goto(self.context.participant_config.session_url.to_string())
            .await
            .context("failed to wait for navigation response")?;

        debug!(participant = %self.participant_name(), "Navigated to page");
        self.context.send_log_message("debug", "Navigated to page");

        wait_for_element(&self.context.page, lite::JOIN_BUTTON, Duration::from_secs(30)).await?;

        self.context
            .find_element(lite::JOIN_BUTTON)
            .await?
            .click()
            .await
            .context("failed to click join button")?;

        debug!(participant = %self.participant_name(), "Clicked on the join button");
        self.context.send_log_message("debug", "Clicked on the join button");

        wait_for_element(&self.context.page, lite::LEAVE_BUTTON, Duration::from_secs(30))
            .await
            .context("We haven't joined the space, cannot find the leave button")?;

        info!(participant = %self.participant_name(), "Joined the space");
        self.context.send_log_message("info", "Joined the space");

        if let Err(err) = self.apply_all_settings().await {
            error!(participant = %self.participant_name(), "Failed to apply settings after joining: {err}");
            self.context
                .send_log_message("error", format!("Failed to apply settings after joining: {err}"));
        }

        self.update_state_inner().await;

        Ok(())
    }

    async fn apply_all_settings(&self) -> Result<()> {
        let Config {
            audio_enabled,
            video_enabled,
            screenshare_enabled,
            ..
        } = &self.context.participant_config.app_config;

        if !audio_enabled {
            self.toggle_audio_inner().await?;
        }

        if !video_enabled {
            self.toggle_video_inner().await?;
        }

        if *screenshare_enabled {
            self.toggle_screen_share_inner().await?;
        }

        Ok(())
    }

    async fn leave_session(&self) -> Result<()> {
        self.leave_button()
            .await?
            .click()
            .await
            .context("Could not click on the leave space button")?;
        info!(participant = %self.participant_name(), "Left the space");
        self.context.send_log_message("info", "Left the space");
        Ok(())
    }

    async fn close_browser(self, mut browser: Browser) -> Result<()> {
        debug!(participant = %self.participant_name(), "Closing the browser...");
        let _ = self.leave_session().await;
        let _ = self.context.page.clone().close().await;
        browser.close().await?;
        browser.wait().await?;
        info!(participant = %self.participant_name(), "Closed the browser");
        self.context.send_log_message("info", "Closed the browser");
        self.context.state.send_modify(|state| {
            state.running = false;
        });
        Ok(())
    }

    async fn toggle_audio_inner(&self) -> Result<()> {
        self.mute_button()
            .await?
            .click()
            .await
            .context("Could not click on the toggle audio button")?;
        info!(participant = %self.participant_name(), "Toggled audio");
        self.context.send_log_message("info", "Toggled audio");
        Ok(())
    }

    async fn toggle_video_inner(&self) -> Result<()> {
        self.camera_button()
            .await?
            .click()
            .await
            .context("Could not click on the toggle camera button")?;
        info!(participant = %self.participant_name(), "Toggled camera");
        self.context.send_log_message("info", "Toggled camera");
        Ok(())
    }

    async fn toggle_screen_share_inner(&self) -> Result<()> {
        self.screen_share_button()
            .await?
            .click()
            .await
            .context("Could not click on the toggle screen share button")?;
        info!(participant = %self.participant_name(), "Toggled screen share");
        self.context.send_log_message("info", "Toggled screen share");
        tokio::time::sleep(Duration::from_secs(1)).await;
        Ok(())
    }

    async fn set_webcam_resolutions_inner(&self, _value: WebcamResolution) -> Result<()> {
        debug!(
            participant = %self.participant_name(),
            "Webcam resolution changes not supported in lite frontend"
        );
        Ok(())
    }

    async fn set_noise_suppression_inner(&self, _value: NoiseSuppression) -> Result<()> {
        debug!(
            participant = %self.participant_name(),
            "Noise suppression changes not supported in lite frontend"
        );
        Ok(())
    }

    async fn toggle_background_blur_inner(&self) -> Result<()> {
        debug!(
            participant = %self.participant_name(),
            "Background blur changes not supported in lite frontend"
        );
        Ok(())
    }

    async fn leave_button(&self) -> Result<Element> {
        self.context.find_element(lite::LEAVE_BUTTON).await
    }

    async fn mute_button(&self) -> Result<Element> {
        self.context.find_element(lite::MUTE_BUTTON).await
    }

    async fn camera_button(&self) -> Result<Element> {
        self.context.find_element(lite::VIDEO_BUTTON).await
    }

    async fn screen_share_button(&self) -> Result<Element> {
        self.context.find_element(lite::SCREEN_SHARE_BUTTON).await
    }

    async fn update_state_inner(&self) {
        let joined = self.leave_button().await.is_ok();
        let mut muted = false;
        let mut video_activated = false;
        let mut screenshare_activated = false;

        if let Ok(mute_button) = self.mute_button().await {
            if let Some(value) = element_state(&mute_button).await {
                muted = !value;
            }
        }
        if let Ok(camera_button) = self.camera_button().await {
            if let Some(value) = element_state(&camera_button).await {
                video_activated = value;
            }
        }
        if let Ok(screen_share_button) = self.screen_share_button().await {
            debug!(participant = %self.participant_name(), "Screen share button: {screen_share_button:?}");
            if let Some(value) = element_state(&screen_share_button).await {
                screenshare_activated = value;
            }
        }

        self.context.state.send_modify(|state| {
            state.joined = joined;
            state.muted = muted;
            state.video_activated = video_activated;
            state.screenshare_activated = screenshare_activated;
            state.transport_mode = TransportMode::default();
            state.webcam_resolution = WebcamResolution::default();
            state.noise_suppression = NoiseSuppression::default();
            state.background_blur = false;
            debug!("Sending state update: {state:?}");
        });
    }
}

impl FrontendDriver for ParticipantInnerLite {
    fn participant_name(&self) -> &str {
        self.context.participant_name()
    }

    fn state(&self) -> &watch::Sender<super::ParticipantState> {
        &self.context.state
    }

    fn log_message(&self, level: &str, message: String) {
        self.context.send_log_message(level, message);
    }

    fn join(&mut self) -> BoxFuture<'_, Result<()>> {
        async move { self.join_session().await }.boxed()
    }

    fn leave(&self) -> BoxFuture<'_, Result<()>> {
        async move { self.leave_session().await }.boxed()
    }

    fn close(self, browser: Browser) -> BoxFuture<'static, Result<()>> {
        async move { self.close_browser(browser).await }.boxed()
    }

    fn toggle_audio(&self) -> BoxFuture<'_, Result<()>> {
        async move { self.toggle_audio_inner().await }.boxed()
    }

    fn toggle_video(&self) -> BoxFuture<'_, Result<()>> {
        async move { self.toggle_video_inner().await }.boxed()
    }

    fn toggle_screen_share(&self) -> BoxFuture<'_, Result<()>> {
        async move { self.toggle_screen_share_inner().await }.boxed()
    }

    fn set_webcam_resolutions(&self, value: WebcamResolution) -> BoxFuture<'_, Result<()>> {
        async move { self.set_webcam_resolutions_inner(value).await }.boxed()
    }

    fn set_noise_suppression(&self, value: NoiseSuppression) -> BoxFuture<'_, Result<()>> {
        async move { self.set_noise_suppression_inner(value).await }.boxed()
    }

    fn toggle_background_blur(&self) -> BoxFuture<'_, Result<()>> {
        async move { self.toggle_background_blur_inner().await }.boxed()
    }

    fn update_state(&self) -> BoxFuture<'_, ()> {
        async move { self.update_state_inner().await }.boxed()
    }
}
