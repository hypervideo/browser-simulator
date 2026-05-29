//! Local DOM automation for the hyper-lite frontend.

use super::{
    super::shared::{
        messages::ParticipantMessage,
        ParticipantState,
    },
    driver::{
        decode_test_state,
        BrowserDriver,
        FrontendAutomation,
        FrontendContext,
    },
    selectors::lite,
};
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
use std::time::{
    Duration,
    Instant,
};

/// Local frontend automation for the hyper-lite UI.
#[derive(Debug)]
pub(super) struct ParticipantInnerLite {
    context: FrontendContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiteEntryPoint {
    InCall,
    Lobby,
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
            .driver
            .goto(self.context.launch_spec.session_url.as_str())
            .await
            .context("failed to wait for navigation response")?;

        debug!(participant = %self.participant_name(), "Navigated to page");
        self.context.send_log_message("debug", "Navigated to page");

        match self.wait_for_entry_point(Duration::from_secs(30)).await? {
            LiteEntryPoint::InCall => {
                debug!(participant = %self.participant_name(), "Lite session is already in-call");
                self.context
                    .send_log_message("debug", "Lite session is already in-call");
            }
            LiteEntryPoint::Lobby => {
                self.prepare_lobby().await?;

                self.context
                    .driver
                    .click(lite::JOIN_BUTTON)
                    .await
                    .context("failed to click join button")?;

                debug!(participant = %self.participant_name(), "Clicked on the join button");
                self.context.send_log_message("debug", "Clicked on the join button");
            }
        }

        self.context
            .driver
            .wait_for(lite::LEAVE_BUTTON, Duration::from_secs(30))
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

    async fn wait_for_entry_point(&self, timeout: Duration) -> Result<LiteEntryPoint> {
        let start = Instant::now();

        loop {
            if self.context.driver.exists(lite::LEAVE_BUTTON).await.unwrap_or(false) {
                return Ok(LiteEntryPoint::InCall);
            }

            if self.context.driver.exists(lite::JOIN_BUTTON).await.unwrap_or(false) {
                return Ok(LiteEntryPoint::Lobby);
            }

            if start.elapsed() > timeout {
                return Err(eyre::eyre!(
                    "timeout waiting for Lite lobby or in-call controls: {} or {}",
                    lite::JOIN_BUTTON,
                    lite::LEAVE_BUTTON
                ));
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn prepare_lobby(&self) -> Result<()> {
        if self.context.driver.exists(lite::NAME_INPUT).await.unwrap_or(false) {
            self.context
                .driver
                .fill(lite::NAME_INPUT, &self.context.launch_spec.username)
                .await
                .context("failed to insert Lite display name")?;

            debug!(participant = %self.participant_name(), "Set the Lite display name");
            self.context.send_log_message(
                "debug",
                format!("Set the Lite display name to {}", self.context.launch_spec.username),
            );
        }

        self.apply_lobby_settings().await
    }

    async fn apply_lobby_settings(&self) -> Result<()> {
        let settings = &self.context.launch_spec.settings;

        if !settings.audio_enabled {
            self.click_if_present(lite::LOBBY_DISABLE_AUDIO_BUTTON, "disable lobby microphone")
                .await?;
        }

        if !settings.video_enabled {
            self.click_if_present(lite::LOBBY_DISABLE_VIDEO_BUTTON, "disable lobby camera")
                .await?;
        }

        Ok(())
    }

    async fn apply_all_settings(&self) -> Result<()> {
        let settings = &self.context.launch_spec.settings;

        self.set_audio_enabled_inner(settings.audio_enabled).await?;
        self.set_video_enabled_inner(settings.video_enabled).await?;

        if settings.screenshare_enabled {
            self.set_screen_share_enabled_inner(true).await?;
        }
        Ok(())
    }

    async fn leave_session(&mut self) -> Result<()> {
        self.context
            .driver
            .click(lite::LEAVE_BUTTON)
            .await
            .context("Could not click on the leave space button")?;

        match self
            .context
            .driver
            .wait_for(lite::LEAVE_CONFIRM_BUTTON, Duration::from_secs(5))
            .await
        {
            Ok(()) => {
                self.context
                    .driver
                    .click(lite::LEAVE_CONFIRM_BUTTON)
                    .await
                    .context("Could not confirm leaving the Lite meeting")?;
                debug!(participant = %self.participant_name(), "Confirmed the Lite leave dialog");
                self.context
                    .send_log_message("debug", "Confirmed the Lite leave dialog");
            }
            Err(err) => {
                debug!(
                    participant = %self.participant_name(),
                    "No Lite leave confirmation appeared, assuming direct leave: {err}"
                );
            }
        }

        info!(participant = %self.participant_name(), "Left the space");
        self.context.send_log_message("info", "Left the space");
        Ok(())
    }

    async fn toggle_audio_inner(&self) -> Result<()> {
        self.context
            .driver
            .click(lite::MUTE_BUTTON)
            .await
            .context("Could not click on the toggle audio button")?;
        info!(participant = %self.participant_name(), "Toggled audio");
        self.context.send_log_message("info", "Toggled audio");
        Ok(())
    }

    async fn toggle_video_inner(&self) -> Result<()> {
        self.context
            .driver
            .click(lite::VIDEO_BUTTON)
            .await
            .context("Could not click on the toggle camera button")?;
        info!(participant = %self.participant_name(), "Toggled camera");
        self.context.send_log_message("info", "Toggled camera");
        Ok(())
    }

    async fn toggle_screen_share_inner(&self) -> Result<()> {
        self.context
            .driver
            .click(lite::SCREEN_SHARE_BUTTON)
            .await
            .context("Could not click on the toggle screen share button")?;
        info!(participant = %self.participant_name(), "Toggled screen share");
        self.context.send_log_message("info", "Toggled screen share");
        tokio::time::sleep(Duration::from_secs(1)).await;
        Ok(())
    }

    async fn set_audio_enabled_inner(&self, enabled: bool) -> Result<()> {
        match self.audio_enabled().await? {
            Some(current) if current == enabled => Ok(()),
            None if enabled => Ok(()),
            _ => self.toggle_audio_inner().await,
        }
    }

    async fn set_video_enabled_inner(&self, enabled: bool) -> Result<()> {
        match self.video_enabled().await? {
            Some(current) if current == enabled => Ok(()),
            None if enabled => Ok(()),
            _ => self.toggle_video_inner().await,
        }
    }

    async fn set_screen_share_enabled_inner(&self, enabled: bool) -> Result<()> {
        match self.screen_share_enabled().await? {
            Some(current) if current == enabled => Ok(()),
            None if !enabled => Ok(()),
            Some(_) | None => self.toggle_screen_share_inner().await,
        }
    }

    async fn toggle_auto_gain_control_inner(&self) -> Result<()> {
        self.log_unsupported("Auto gain control");
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
        self.log_unsupported("Noise suppression");
        Ok(())
    }

    async fn toggle_background_blur_inner(&self) -> Result<()> {
        self.log_unsupported("Background blur");
        Ok(())
    }

    async fn click_if_present(&self, selector: &str, action: &str) -> Result<bool> {
        if !self.context.driver.exists(selector).await.unwrap_or(false) {
            debug!(
                participant = %self.participant_name(),
                "Could not find Lite control for {action}: {selector}"
            );
            return Ok(false);
        }

        self.context
            .driver
            .click(selector)
            .await
            .with_context(|| format!("Could not click Lite control for {action}: {selector}"))?;
        debug!(participant = %self.participant_name(), "Clicked Lite control for {action}");
        Ok(true)
    }

    fn log_unsupported(&self, feature: &str) {
        debug!(
            participant = %self.participant_name(),
            "{feature} changes not supported in lite frontend"
        );
        self.context
            .send_log_message("debug", format!("{feature} changes not supported in lite frontend"));
    }

    async fn audio_enabled(&self) -> Result<Option<bool>> {
        let driver = self.context.driver.as_ref();

        Ok(audio_enabled_from_button_state(
            decode_test_state(driver.attribute(lite::MUTE_BUTTON, "data-test-state").await?),
            aria_pressed(driver, lite::MUTE_BUTTON).await,
            aria_label(driver, lite::MUTE_BUTTON).await.as_deref(),
        ))
    }

    async fn video_enabled(&self) -> Result<Option<bool>> {
        let driver = self.context.driver.as_ref();

        Ok(video_enabled_from_button_state(
            decode_test_state(driver.attribute(lite::VIDEO_BUTTON, "data-test-state").await?),
            aria_pressed(driver, lite::VIDEO_BUTTON).await,
            aria_label(driver, lite::VIDEO_BUTTON).await.as_deref(),
        ))
    }

    async fn screen_share_enabled(&self) -> Result<Option<bool>> {
        let driver = self.context.driver.as_ref();
        let data_test_state = driver.attribute(lite::SCREEN_SHARE_BUTTON, "data-test-state").await?;

        Ok(decode_test_state(data_test_state).or(aria_pressed(driver, lite::SCREEN_SHARE_BUTTON).await))
    }

    async fn refresh_state_inner(&self) -> Result<ParticipantState> {
        let joined = self.context.driver.exists(lite::LEAVE_BUTTON).await.unwrap_or(false);
        let mut state = ParticipantState {
            username: self.context.launch_spec.username.clone(),
            running: true,
            joined,
            auto_gain_control: self.context.launch_spec.settings.auto_gain_control,
            transport_mode: TransportMode::default(),
            webcam_resolution: WebcamResolution::default(),
            noise_suppression: NoiseSuppression::default(),
            muted: !self.context.launch_spec.settings.audio_enabled,
            video_activated: self.context.launch_spec.settings.video_enabled,
            ..Default::default()
        };

        if let Ok(Some(value)) = self.audio_enabled().await {
            state.muted = !value;
        }

        if let Ok(Some(value)) = self.video_enabled().await {
            state.video_activated = value;
        }

        if let Ok(Some(value)) = self.screen_share_enabled().await {
            state.screenshare_activated = value;
        }

        Ok(state)
    }
}

async fn aria_pressed(driver: &dyn BrowserDriver, selector: &str) -> Option<bool> {
    driver
        .attribute(selector, "aria-pressed")
        .await
        .ok()
        .flatten()
        .and_then(|value| value.parse().ok())
}

async fn aria_label(driver: &dyn BrowserDriver, selector: &str) -> Option<String> {
    driver.attribute(selector, "aria-label").await.ok().flatten()
}

fn audio_enabled_from_button_state(
    data_test_state: Option<bool>,
    aria_pressed: Option<bool>,
    aria_label: Option<&str>,
) -> Option<bool> {
    data_test_state
        .or_else(|| aria_pressed.map(|pressed| !pressed))
        .or_else(|| {
            aria_label.and_then(|label| {
                if label.contains("Unmute") {
                    Some(false)
                } else if label.contains("Mute") {
                    Some(true)
                } else {
                    None
                }
            })
        })
}

fn video_enabled_from_button_state(
    data_test_state: Option<bool>,
    aria_pressed: Option<bool>,
    aria_label: Option<&str>,
) -> Option<bool> {
    data_test_state
        .or_else(|| aria_pressed.map(|pressed| !pressed))
        .or_else(|| {
            aria_label.and_then(|label| {
                if label.contains("Turn on") || label.contains("Turn video on") {
                    Some(false)
                } else if label.contains("Turn off") || label.contains("Turn video off") {
                    Some(true)
                } else {
                    None
                }
            })
        })
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

#[cfg(test)]
mod tests {
    use super::{
        audio_enabled_from_button_state,
        video_enabled_from_button_state,
    };

    #[test]
    fn audio_state_prefers_legacy_data_test_state() {
        assert_eq!(
            audio_enabled_from_button_state(Some(true), Some(true), Some("Unmute microphone")),
            Some(true)
        );
        assert_eq!(
            audio_enabled_from_button_state(Some(false), Some(false), Some("Mute microphone")),
            Some(false)
        );
    }

    #[test]
    fn audio_state_maps_current_aria_pressed_to_enabled() {
        assert_eq!(
            audio_enabled_from_button_state(None, Some(false), Some("Mute")),
            Some(true)
        );
        assert_eq!(
            audio_enabled_from_button_state(None, Some(true), Some("Unmute microphone")),
            Some(false)
        );
    }

    #[test]
    fn audio_state_falls_back_to_current_mobile_labels() {
        assert_eq!(
            audio_enabled_from_button_state(None, None, Some("Mute microphone")),
            Some(true)
        );
        assert_eq!(
            audio_enabled_from_button_state(None, None, Some("Unmute microphone")),
            Some(false)
        );
    }

    #[test]
    fn video_state_prefers_legacy_data_test_state() {
        assert_eq!(
            video_enabled_from_button_state(Some(true), Some(true), Some("Turn on camera")),
            Some(true)
        );
        assert_eq!(
            video_enabled_from_button_state(Some(false), Some(false), Some("Turn off camera")),
            Some(false)
        );
    }

    #[test]
    fn video_state_maps_current_aria_pressed_to_enabled() {
        assert_eq!(
            video_enabled_from_button_state(None, Some(false), Some("Video")),
            Some(true)
        );
        assert_eq!(
            video_enabled_from_button_state(None, Some(true), Some("Video")),
            Some(false)
        );
    }

    #[test]
    fn video_state_falls_back_to_current_labels() {
        assert_eq!(
            video_enabled_from_button_state(None, None, Some("Turn off camera")),
            Some(true)
        );
        assert_eq!(
            video_enabled_from_button_state(None, None, Some("Turn on camera")),
            Some(false)
        );
        assert_eq!(
            video_enabled_from_button_state(None, None, Some("Turn video off")),
            Some(true)
        );
        assert_eq!(
            video_enabled_from_button_state(None, None, Some("Turn video on")),
            Some(false)
        );
    }
}
