//! Local DOM automation for the hyper-lite frontend.

use super::{
    super::shared::{
        messages::ParticipantMessage,
        ParticipantState,
    },
    commands::{
        get_auto_gain_control,
        set_auto_gain_control,
    },
    frontend::{
        element_state,
        FrontendAutomation,
        FrontendContext,
    },
};
use crate::{
    participant::local::selectors::lite,
    util::wait_for_element,
};
use chromiumoxide::Element;
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

/// Local frontend automation for the hyper-lite UI.
#[derive(Debug)]
pub(super) struct ParticipantInnerLite {
    context: FrontendContext,
}

impl ParticipantInnerLite {
    pub(super) fn new(context: FrontendContext) -> Self {
        Self { context }
    }

    fn participant_name(&self) -> &str {
        self.context.participant_name()
    }

    async fn join_session(&mut self) -> Result<()> {
        self.context
            .page
            .goto(self.context.launch_spec.session_url.to_string())
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
            error!(
                participant = %self.participant_name(),
                "Failed to apply settings after joining: {err}"
            );
            self.context
                .send_log_message("error", format!("Failed to apply settings after joining: {err}"));
        }

        Ok(())
    }

    async fn apply_all_settings(&self) -> Result<()> {
        let settings = &self.context.launch_spec.settings;

        set_auto_gain_control(&self.context.page, settings.auto_gain_control)
            .await
            .context("failed to set auto gain control")?;

        if !settings.audio_enabled {
            self.toggle_audio_inner().await?;
        }

        if !settings.video_enabled {
            self.toggle_video_inner().await?;
        }

        if settings.screenshare_enabled {
            self.toggle_screen_share_inner().await?;
        }

        Ok(())
    }

    async fn leave_session(&mut self) -> Result<()> {
        self.leave_button()
            .await?
            .click()
            .await
            .context("Could not click on the leave space button")?;
        info!(participant = %self.participant_name(), "Left the space");
        self.context.send_log_message("info", "Left the space");
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

    async fn toggle_auto_gain_control_inner(&self) -> Result<()> {
        let auto_gain_control = get_auto_gain_control(&self.context.page).await?;
        set_auto_gain_control(&self.context.page, !auto_gain_control)
            .await
            .context("Failed to set auto gain control")?;
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

    async fn refresh_state_inner(&self) -> Result<ParticipantState> {
        let joined = self.leave_button().await.is_ok();
        let mut state = ParticipantState {
            username: self.context.launch_spec.username.clone(),
            running: true,
            joined,
            auto_gain_control: self.context.launch_spec.settings.auto_gain_control,
            transport_mode: TransportMode::default(),
            webcam_resolution: WebcamResolution::default(),
            noise_suppression: NoiseSuppression::default(),
            ..Default::default()
        };

        if let Ok(value) = get_auto_gain_control(&self.context.page).await {
            state.auto_gain_control = value;
        }

        if let Ok(mute_button) = self.mute_button().await {
            if let Some(value) = element_state(&mute_button).await {
                state.muted = !value;
            }
        }
        if let Ok(camera_button) = self.camera_button().await {
            if let Some(value) = element_state(&camera_button).await {
                state.video_activated = value;
            }
        }
        if let Ok(screen_share_button) = self.screen_share_button().await {
            debug!(participant = %self.participant_name(), "Screen share button: {screen_share_button:?}");
            if let Some(value) = element_state(&screen_share_button).await {
                state.screenshare_activated = value;
            }
        }

        Ok(state)
    }
}

impl FrontendAutomation for ParticipantInnerLite {
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
