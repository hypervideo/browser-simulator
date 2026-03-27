//! Browser interaction for the hyper.video ("hyper core") frontend.

use super::{
    commands::{
        get_force_webrtc,
        get_noise_suppression,
        get_outgoing_camera_resolution,
        set_noise_suppression,
    },
    frontend::{
        element_state,
        run_local_participant,
        DriverContext,
        FrontendDriver,
    },
    messages::ParticipantMessage,
};
use crate::{
    auth::{
        BorrowedCookie,
        HyperSessionCookieManger,
    },
    participant::{
        commands::{
            get_background_blur,
            set_background_blur,
            set_force_webrtc,
            set_outgoing_camera_resolution,
        },
        selectors::classic,
    },
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

/// Frontend driver for the hyper.video ("hyper core") UI.
#[derive(Debug)]
pub(super) struct ParticipantInner {
    context: DriverContext,
    auth: BorrowedCookie,
}

impl ParticipantInner {
    #[instrument(level = "debug", skip_all, fields(name = %participant_config.username))]
    pub(super) async fn run(
        participant_config: ParticipantConfig,
        cookie: Option<BorrowedCookie>,
        cookie_manager: HyperSessionCookieManger,
        receiver: UnboundedReceiver<ParticipantMessage>,
        sender: UnboundedSender<super::messages::ParticipantLogMessage>,
        state: watch::Sender<super::ParticipantState>,
    ) -> Result<()> {
        let auth = if let Some(cookie) = cookie {
            cookie
        } else {
            cookie_manager
                .fetch_new_cookie(participant_config.base_url(), &participant_config.username)
                .await?
        };

        run_local_participant(participant_config, receiver, sender, state, move |context| Self {
            context,
            auth,
        })
        .await
    }

    async fn set_cookie(&self) -> Result<()> {
        let domain = self
            .context
            .participant_config
            .session_url
            .host_str()
            .unwrap_or("localhost");
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

    async fn join_session(&mut self) -> Result<()> {
        if self.context.state.borrow().joined {
            warn!(participant = %self.participant_name(), "Already joined.");
            self.context.send_log_message("warn", "Already joined.");
            return Ok(());
        }

        self.set_cookie().await?;

        self.context
            .page
            .goto(self.context.participant_config.session_url.to_string())
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
            .type_str(&self.context.participant_config.username)
            .await
            .context("failed to insert name")?;

        debug!(participant = %self.participant_name(), "Set the name of the participant");
        self.context.send_log_message(
            "debug",
            format!(
                "Set the name of the participant to {}",
                self.context.participant_config.username
            ),
        );

        wait_for_element(&self.context.page, classic::JOIN_BUTTON, Duration::from_secs(30)).await?;

        if let Err(err) = self.apply_all_settings(true).await {
            error!(participant = %self.participant_name(), "Failed to apply settings before joining: {err}");
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

        self.update_state_inner().await;

        Ok(())
    }

    async fn apply_all_settings(&self, in_lobby: bool) -> Result<()> {
        let Config {
            noise_suppression,
            transport,
            resolution,
            blur,
            audio_enabled,
            video_enabled,
            screenshare_enabled,
            ..
        } = &self.context.participant_config.app_config;

        set_noise_suppression(&self.context.page, *noise_suppression)
            .await
            .context("failed to set noise suppression")?;
        set_background_blur(&self.context.page, *blur)
            .await
            .context("failed to set background blur")?;
        set_outgoing_camera_resolution(&self.context.page, resolution)
            .await
            .context("failed to set outgoing camera resolution")?;
        set_force_webrtc(&self.context.page, transport == &TransportMode::WebRTC)
            .await
            .context("failed to set transport mode")?;

        if !audio_enabled {
            self.toggle_audio_inner().await?;
        }

        if !video_enabled {
            self.toggle_video_inner().await?;
        }

        if !in_lobby && *screenshare_enabled {
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

        if let Err(err) = self.leave_session().await {
            error!(
                participant = %self.participant_name(),
                "Failed leaving space while closing browser: {err}"
            );
            self.context
                .send_log_message("error", format!("Failed leaving space while closing browser: {err}"));
        }

        if let Err(err) = self.context.page.clone().close().await {
            error!(participant = %self.participant_name(), "Error closing page: {err}");
            self.context
                .send_log_message("error", format!("Error closing page: {err}"));
        }

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

    async fn update_state_inner(&self) {
        let joined = self.leave_button().await.is_ok();
        let mut muted = false;
        let mut video_activated = false;
        let mut transport_mode = TransportMode::default();
        let mut webcam_resolution = WebcamResolution::default();
        let mut noise_suppression = NoiseSuppression::default();
        let mut background_blur = false;
        let mut screenshare_activated = false;

        if let Ok(value) = get_noise_suppression(&self.context.page).await {
            noise_suppression = value;
        }

        if let Ok(mute_button) = self.mute_button().await {
            if let Some(value) = element_state(&mute_button).await {
                muted = !value;
            }
        }

        if let Ok(camera_button) = self.camera_button().await {
            if let Some(element_state) = element_state(&camera_button).await {
                video_activated = element_state;
            }
        }

        if let Ok(screen_share_button) = self.screen_share_button().await {
            if let Some(element_state) = element_state(&screen_share_button).await {
                screenshare_activated = element_state;
            }
        }

        if let Ok(value) = get_force_webrtc(&self.context.page).await {
            if value {
                transport_mode = TransportMode::WebRTC;
            }
        }

        if let Ok(value) = get_outgoing_camera_resolution(&self.context.page).await {
            webcam_resolution = value;
        }

        if let Ok(blur) = get_background_blur(&self.context.page).await {
            background_blur = blur;
        }

        self.context.state.send_modify(|state| {
            state.joined = joined;
            state.muted = muted;
            state.video_activated = video_activated;
            state.transport_mode = transport_mode;
            state.webcam_resolution = webcam_resolution;
            state.noise_suppression = noise_suppression;
            state.background_blur = background_blur;
            state.screenshare_activated = screenshare_activated;
            debug!("Sending state update: {state:?}");
        });
    }
}

impl FrontendDriver for ParticipantInner {
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
