//! Local DOM automation for the hyper.video ("hyper core") frontend.

use super::{
    super::shared::{
        messages::ParticipantMessage,
        ParticipantState,
    },
    commands::{
        get_background_blur,
        get_force_webrtc,
        get_noise_suppression,
        get_outgoing_camera_resolution,
        set_background_blur,
        set_force_webrtc,
        set_noise_suppression,
        set_outgoing_camera_resolution,
    },
    frontend::{
        element_state,
        FrontendAutomation,
        FrontendContext,
    },
};
use crate::{
    auth::BorrowedCookie,
    participant::local::selectors::classic,
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
        let domain = self.context.launch_spec.session_url.host_str().unwrap_or("localhost");
        let cookie = self.auth.as_browser_cookie_for(domain)?;

        self.context
            .page
            .set_cookies(vec![cookie])
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
            .page
            .goto(self.context.launch_spec.session_url.to_string())
            .await
            .context("failed to wait for navigation response")?;

        debug!(participant = %self.participant_name(), "Navigated to page");
        self.context.send_log_message("debug", "Navigated to page");

        let input = wait_for_element(&self.context.page, classic::NAME_INPUT, Duration::from_secs(30))
            .await
            .context("failed to find input name field")?;
        input
            .focus()
            .await
            .context("failed to focus on the name input")?
            .call_js_fn("function() { this.value = ''; }", true)
            .await
            .context("failed to empty current name")?;
        input
            .type_str(&self.context.launch_spec.username)
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

        wait_for_element(&self.context.page, classic::JOIN_BUTTON, Duration::from_secs(30)).await?;

        if let Err(err) = self.apply_all_settings(true).await {
            error!(
                participant = %self.participant_name(),
                "Failed to apply settings before joining: {err}"
            );
        }

        self.context
            .find_element(classic::JOIN_BUTTON)
            .await?
            .click()
            .await
            .context("failed to click join button")?;

        debug!(participant = %self.participant_name(), "Clicked on the join button");
        self.context.send_log_message("debug", "Clicked on the join button");

        wait_for_element(&self.context.page, classic::LEAVE_BUTTON, Duration::from_secs(30))
            .await
            .context("We haven't joined the space, cannot find the leave button")?;

        info!(participant = %self.participant_name(), "Joined the space");
        self.context.send_log_message("info", "Joined the space");

        Ok(())
    }

    async fn apply_all_settings(&self, in_lobby: bool) -> Result<()> {
        let settings = &self.context.launch_spec.settings;

        set_noise_suppression(&self.context.page, settings.noise_suppression)
            .await
            .context("failed to set noise suppression")?;
        set_background_blur(&self.context.page, settings.blur)
            .await
            .context("failed to set background blur")?;
        set_outgoing_camera_resolution(&self.context.page, &settings.resolution)
            .await
            .context("failed to set outgoing camera resolution")?;
        set_force_webrtc(&self.context.page, settings.transport == TransportMode::WebRTC)
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
            .context("Could not click on the toggle screen share button")
            .map(|_| ())
    }

    async fn set_webcam_resolutions_inner(&self, value: WebcamResolution) -> Result<()> {
        debug!(participant = %self.participant_name(), "Changing to {value} resolution");

        set_outgoing_camera_resolution(&self.context.page, &value)
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

        set_noise_suppression(&self.context.page, value)
            .await
            .context("Failed to set noise suppression level")?;

        Ok(())
    }

    async fn toggle_background_blur_inner(&self) -> Result<()> {
        let background_blur = get_background_blur(&self.context.page).await?;
        set_background_blur(&self.context.page, !background_blur)
            .await
            .context("Failed to set background blur")?;
        Ok(())
    }

    async fn leave_button(&self) -> Result<Element> {
        self.context.find_element(classic::LEAVE_BUTTON).await
    }

    async fn mute_button(&self) -> Result<Element> {
        self.context.find_element(classic::MUTE_BUTTON).await
    }

    async fn camera_button(&self) -> Result<Element> {
        self.context.find_element(classic::VIDEO_BUTTON).await
    }

    async fn screen_share_button(&self) -> Result<Element> {
        self.context.find_element(classic::SCREEN_SHARE_BUTTON).await
    }

    async fn refresh_state_inner(&self) -> Result<ParticipantState> {
        let joined = self.leave_button().await.is_ok();
        let mut state = ParticipantState {
            username: self.context.launch_spec.username.clone(),
            running: true,
            joined,
            ..Default::default()
        };

        if let Ok(value) = get_noise_suppression(&self.context.page).await {
            state.noise_suppression = value;
        }

        if let Ok(mute_button) = self.mute_button().await {
            if let Some(value) = element_state(&mute_button).await {
                state.muted = !value;
            }
        }

        if let Ok(camera_button) = self.camera_button().await {
            if let Some(element_state) = element_state(&camera_button).await {
                state.video_activated = element_state;
            }
        }

        if let Ok(screen_share_button) = self.screen_share_button().await {
            if let Some(element_state) = element_state(&screen_share_button).await {
                state.screenshare_activated = element_state;
            }
        }

        if let Ok(value) = get_force_webrtc(&self.context.page).await {
            if value {
                state.transport_mode = TransportMode::WebRTC;
            }
        }

        if let Ok(value) = get_outgoing_camera_resolution(&self.context.page).await {
            state.webcam_resolution = value;
        }

        if let Ok(blur) = get_background_blur(&self.context.page).await {
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
