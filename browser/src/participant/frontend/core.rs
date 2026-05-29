//! Local DOM automation for the hyper.video ("hyper core") frontend.

use super::{
    super::shared::{
        messages::ParticipantMessage,
        ParticipantState,
    },
    commands::{
        get_auto_gain_control,
        get_background_blur,
        get_force_webrtc,
        get_noise_suppression,
        get_outgoing_camera_resolution,
        set_auto_gain_control,
        set_background_blur,
        set_force_webrtc,
        set_noise_suppression,
        set_outgoing_camera_resolution,
    },
    driver::{
        decode_test_state,
        FrontendAutomation,
        FrontendContext,
    },
    selectors::classic,
};
use crate::auth::BorrowedCookie;
use client_simulator_config::{
    NoiseSuppression,
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

/// Local frontend automation for the hyper.video ("hyper core") UI.
#[derive(Debug)]
pub(super) struct ParticipantInner {
    context: FrontendContext,
    auth: BorrowedCookie,
}

impl ParticipantInner {
    pub(super) fn new(context: FrontendContext, auth: BorrowedCookie) -> Self {
        Self { context, auth }
    }

    async fn set_cookie(&self) -> Result<()> {
        let domain = self
            .context
            .launch_spec
            .session_url
            .host_str()
            .unwrap_or("localhost")
            .to_owned();
        let value = self.auth.raw_value().to_owned();
        self.context
            .driver
            .set_cookie(&domain, "hyper_session", &value)
            .await
            .context("failed to set cookie")?;

        debug!(participant = %self.participant_name(), "Set cookie");
        self.context
            .send_log_message("debug", format!("Set cookie for domain {domain}"));

        Ok(())
    }

    fn participant_name(&self) -> &str {
        self.context.participant_name()
    }

    async fn join_session(&mut self) -> Result<()> {
        self.set_cookie().await?;

        self.context
            .driver
            .goto(self.context.launch_spec.session_url.as_str())
            .await
            .context("failed to wait for navigation response")?;

        debug!(participant = %self.participant_name(), "Navigated to page");
        self.context.send_log_message("debug", "Navigated to page");

        self.context
            .driver
            .wait_for(classic::NAME_INPUT, Duration::from_secs(30))
            .await
            .context("failed to find input name field")?;
        self.context
            .driver
            .fill(classic::NAME_INPUT, &self.context.launch_spec.username)
            .await
            .context("failed to insert name")?;

        debug!(participant = %self.participant_name(), "Set the name of the participant");
        self.context.send_log_message(
            "debug",
            format!(
                "Set the name of the participant to {}",
                self.context.launch_spec.username
            ),
        );

        self.context
            .driver
            .wait_for(classic::JOIN_BUTTON, Duration::from_secs(30))
            .await?;

        if let Err(err) = self.apply_all_settings(true).await {
            error!(
                participant = %self.participant_name(),
                "Failed to apply settings before joining: {err}"
            );
        }

        self.context
            .driver
            .click(classic::JOIN_BUTTON)
            .await
            .context("failed to click join button")?;

        debug!(participant = %self.participant_name(), "Clicked on the join button");
        self.context.send_log_message("debug", "Clicked on the join button");

        self.context
            .driver
            .wait_for(classic::LEAVE_BUTTON, Duration::from_secs(30))
            .await
            .context("We haven't joined the space, cannot find the leave button")?;

        info!(participant = %self.participant_name(), "Joined the space");
        self.context.send_log_message("info", "Joined the space");

        Ok(())
    }

    async fn apply_all_settings(&self, in_lobby: bool) -> Result<()> {
        let settings = &self.context.launch_spec.settings;
        let driver = self.context.driver.as_ref();

        set_auto_gain_control(driver, settings.auto_gain_control)
            .await
            .context("failed to set auto gain control")?;
        set_noise_suppression(driver, settings.noise_suppression)
            .await
            .context("failed to set noise suppression")?;
        set_background_blur(driver, settings.blur)
            .await
            .context("failed to set background blur")?;
        set_outgoing_camera_resolution(driver, &settings.resolution)
            .await
            .context("failed to set outgoing camera resolution")?;
        set_force_webrtc(driver, settings.transport == TransportMode::WebRTC)
            .await
            .context("failed to set transport mode")?;

        if !settings.audio_enabled {
            self.toggle_audio_inner().await?;
        }

        if !settings.video_enabled {
            self.toggle_video_inner().await?;
        }

        if !in_lobby && settings.screenshare_enabled {
            self.toggle_screen_share_inner().await?;
        }

        Ok(())
    }

    async fn leave_session(&mut self) -> Result<()> {
        self.context
            .driver
            .click(classic::LEAVE_BUTTON)
            .await
            .context("Could not click on the leave space button")?;

        info!(participant = %self.participant_name(), "Left the space");
        self.context.send_log_message("info", "Left the space");

        Ok(())
    }

    async fn toggle_audio_inner(&self) -> Result<()> {
        self.context
            .driver
            .click(classic::MUTE_BUTTON)
            .await
            .context("Could not click on the toggle audio button")?;

        info!(participant = %self.participant_name(), "Toggled audio");
        self.context.send_log_message("info", "Toggled audio");

        Ok(())
    }

    async fn toggle_video_inner(&self) -> Result<()> {
        self.context
            .driver
            .click(classic::VIDEO_BUTTON)
            .await
            .context("Could not click on the toggle camera button")?;

        info!(participant = %self.participant_name(), "Toggled camera");
        self.context.send_log_message("info", "Toggled camera");

        Ok(())
    }

    async fn toggle_screen_share_inner(&self) -> Result<()> {
        self.context
            .driver
            .click(classic::SCREEN_SHARE_BUTTON)
            .await
            .context("Could not click on the toggle screen share button")
            .map(|_| ())
    }

    async fn set_webcam_resolutions_inner(&self, value: WebcamResolution) -> Result<()> {
        debug!(participant = %self.participant_name(), "Changing to {value} resolution");

        set_outgoing_camera_resolution(self.context.driver.as_ref(), &value)
            .await
            .context("Failed to set outgoing camera resolution")?;

        Ok(())
    }

    async fn set_noise_suppression_inner(&self, value: NoiseSuppression) -> Result<()> {
        info!(
            participant = %self.participant_name(),
            "Changing noise suppression to {value}"
        );
        self.context
            .send_log_message("info", format!("Changing noise suppression to {value}"));

        set_noise_suppression(self.context.driver.as_ref(), value)
            .await
            .context("Failed to set noise suppression level")?;

        Ok(())
    }

    async fn toggle_auto_gain_control_inner(&self) -> Result<()> {
        let driver = self.context.driver.as_ref();
        let auto_gain_control = get_auto_gain_control(driver).await?;
        set_auto_gain_control(driver, !auto_gain_control)
            .await
            .context("Failed to set auto gain control")?;
        Ok(())
    }

    async fn toggle_background_blur_inner(&self) -> Result<()> {
        let driver = self.context.driver.as_ref();
        let background_blur = get_background_blur(driver).await?;
        set_background_blur(driver, !background_blur)
            .await
            .context("Failed to set background blur")?;
        Ok(())
    }

    async fn refresh_state_inner(&self) -> Result<ParticipantState> {
        let driver = self.context.driver.as_ref();
        let joined = driver.exists(classic::LEAVE_BUTTON).await.unwrap_or(false);
        let mut state = ParticipantState {
            username: self.context.launch_spec.username.clone(),
            running: true,
            joined,
            ..Default::default()
        };

        if let Ok(value) = get_noise_suppression(driver).await {
            state.noise_suppression = value;
        }

        if let Ok(value) = get_auto_gain_control(driver).await {
            state.auto_gain_control = value;
        }

        if let Ok(value) = driver.attribute(classic::MUTE_BUTTON, "data-test-state").await {
            if let Some(active) = decode_test_state(value) {
                state.muted = !active;
            }
        }

        if let Ok(value) = driver.attribute(classic::VIDEO_BUTTON, "data-test-state").await {
            if let Some(active) = decode_test_state(value) {
                state.video_activated = active;
            }
        }

        if let Ok(value) = driver.attribute(classic::SCREEN_SHARE_BUTTON, "data-test-state").await {
            if let Some(active) = decode_test_state(value) {
                state.screenshare_activated = active;
            }
        }

        if let Ok(value) = get_force_webrtc(driver).await {
            if value {
                state.transport_mode = TransportMode::WebRTC;
            }
        }

        if let Ok(value) = get_outgoing_camera_resolution(driver).await {
            state.webcam_resolution = value;
        }

        if let Ok(blur) = get_background_blur(driver).await {
            state.background_blur = blur;
        }

        Ok(state)
    }
}

impl FrontendAutomation for ParticipantInner {
    fn join(&mut self) -> BoxFuture<'_, Result<()>> {
        async move { self.join_session().await }.boxed()
    }

    fn leave(&mut self) -> BoxFuture<'_, Result<()>> {
        async move { self.leave_session().await }.boxed()
    }

    fn handle_command(&mut self, message: ParticipantMessage) -> BoxFuture<'_, Result<()>> {
        async move {
            match message {
                ParticipantMessage::Join => self.join_session().await,
                ParticipantMessage::Leave => self.leave_session().await,
                ParticipantMessage::Close => Ok(()),
                ParticipantMessage::ToggleAudio => self.toggle_audio_inner().await,
                ParticipantMessage::ToggleVideo => self.toggle_video_inner().await,
                ParticipantMessage::ToggleScreenshare => self.toggle_screen_share_inner().await,
                ParticipantMessage::ToggleAutoGainControl => self.toggle_auto_gain_control_inner().await,
                ParticipantMessage::SetWebcamResolutions(value) => self.set_webcam_resolutions_inner(value).await,
                ParticipantMessage::SetNoiseSuppression(value) => self.set_noise_suppression_inner(value).await,
                ParticipantMessage::ToggleBackgroundBlur => self.toggle_background_blur_inner().await,
            }
        }
        .boxed()
    }

    fn refresh_state(&mut self) -> BoxFuture<'_, Result<ParticipantState>> {
        async move { self.refresh_state_inner().await }.boxed()
    }
}
