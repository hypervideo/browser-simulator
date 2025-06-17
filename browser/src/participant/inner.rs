use super::{
    commands::{
        get_force_webrtc,
        get_noise_suppression,
        get_outgoing_camera_resolution,
        set_noise_suppression,
    },
    messages::ParticipantMessage,
    ParticipantState,
};
use crate::{
    auth::{
        BorrowedCookie,
        HyperSessionCookieManger,
    },
    create_browser,
    participant::commands::{
        get_background_blur,
        set_background_blur,
        set_force_webrtc,
        set_outgoing_camera_resolution,
    },
    wait_for_element,
};
use chromiumoxide::{
    cdp::browser_protocol::target::CreateTargetParams,
    Browser,
    Element,
    Handler,
    Page,
};
use client_simulator_config::{
    BrowserConfig,
    Config,
    NoiseSuppression,
    ParticipantConfig,
    TransportMode,
    WebcamResolution,
};
use eyre::{
    bail,
    Context as _,
    ContextCompat as _,
    Result,
};
use futures::StreamExt as _;
use std::time::Duration;
use tokio::{
    sync::{
        mpsc::UnboundedReceiver,
        watch,
    },
    task::JoinHandle,
};

/// Async participant "worker" that holds the browser and session.
/// It has the direct command to the browser session that can modify the
/// participant behavior in the space.
/// It holds the message receiver that is used to handle the incoming messages
/// from the sync TUI runtime.
#[derive(Debug)]
pub(super) struct ParticipantInner {
    participant_config: ParticipantConfig,
    page: Page,
    state: watch::Sender<ParticipantState>,
    auth: BorrowedCookie,
}

impl ParticipantInner {
    #[instrument(level = "debug", skip_all, fields(name = %participant_config.username))]
    pub(super) async fn run(
        participant_config: ParticipantConfig,
        cookie: Option<BorrowedCookie>,
        cookie_manager: HyperSessionCookieManger,
        receiver: UnboundedReceiver<ParticipantMessage>,
        state: watch::Sender<ParticipantState>,
    ) -> Result<()> {
        let auth = if let Some(cookie) = cookie {
            cookie
        } else {
            cookie_manager
                .fetch_new_cookie(participant_config.base_url(), &participant_config.username)
                .await?
        };

        let (mut browser, handler) = create_browser(&BrowserConfig::from(&participant_config)).await?;

        let browser_event_task_handle = Self::drive_browser_events(&participant_config.username, handler);

        let page = Self::create_page_retry(&participant_config, &mut browser).await?;

        let participant = Self {
            participant_config,
            page,
            state: state.clone(),
            auth,
        };

        participant
            .handle_actions(browser, receiver)
            .await
            .context("failed to handle actions")?;

        browser_event_task_handle.await?;

        Ok(())
    }

    fn drive_browser_events(name: impl ToString, mut handler: Handler) -> JoinHandle<()> {
        let name = name.to_string();
        tokio::task::spawn(async move {
            while let Some(event) = handler.next().await {
                if let Err(err) = event {
                    if err.to_string().contains("ResetWithoutClosingHandshake") {
                        error!(name, "Browser unexpectedly closed");
                        break;
                    }
                    error!(name, "error in browser handler: {err:?}");
                }
            }
            debug!(name, "Browser event handler stopped");
        })
    }

    async fn handle_actions(
        mut self,
        mut browser: Browser,
        mut receiver: UnboundedReceiver<ParticipantMessage>,
    ) -> Result<()> {
        self.state.send_modify(|state| {
            state.running = true;
        });

        if let Err(err) = self.join().await {
            error!("Failed joining the session when starting the browser {err}");

            match browser.kill().await {
                Some(Ok(_)) => debug!("browser killed"),
                Some(Err(err)) => error!("failed to kill browser: {err}"),
                None => debug!("browser process not found"),
            };
            self.state.send_modify(|state| {
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

                // Event is fired when the page is closed by the user
                Some(_) = detached_event.next() => {
                    warn!(self.participant_config.username, "Browser unexpectedly closed");
                    match browser.kill().await {
                        Some(Ok(_)) => debug!("browser killed"),
                        Some(Err(err)) => error!("failed to kill browser: {err}"),
                        None => debug!("browser process not found"),
                    }
                    break;
                },

                Some(message) = receiver.recv() => {
                    message
                }
            };

            if let Err(e) = match message {
                ParticipantMessage::Join => self.join().await,
                ParticipantMessage::Leave => self.leave().await,
                ParticipantMessage::Close => {
                    self.close(browser).await?;
                    return Ok(());
                }
                ParticipantMessage::ToggleAudio => self.toggle_audio().await,
                ParticipantMessage::ToggleVideo => self.toggle_video().await,
                ParticipantMessage::SetWebcamResolutions(value) => self.set_webcam_resolutions(value).await,
                ParticipantMessage::SetNoiseSuppression(value) => self.set_noise_suppression(value).await,
                ParticipantMessage::ToggleBackgroundBlur => self.toggle_background_blur().await,
            } {
                error!("Running action {message} failed with error: {e}.");
            }

            self.update_state().await;
        }

        self.state.send_modify(|state| {
            state.running = false;
        });

        Ok(())
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

        debug!(config.username, "Created a new page for the {}", config.session_url);

        Ok(page)
    }

    async fn create_page_retry(config: &ParticipantConfig, browser: &mut Browser) -> Result<Page> {
        let mut backoff = hyper_video_maybe_backoff::MaybeBackoff::default();
        let mut attempt = 0;
        loop {
            backoff.sleep().await;
            match Self::create_page(config, browser).await {
                Ok(page) => return Ok(page),
                Err(_) if attempt < 5 => {
                    attempt += 1;
                    backoff.arm();
                    warn!(?attempt, "Failed to create a new page, retrying...");
                }
                Err(err) => return Err(err),
            }
        }
    }
}

impl ParticipantInner {
    async fn set_cookie(&self) -> Result<()> {
        let domain = self.participant_config.session_url.host_str().unwrap_or("localhost");

        let cookie = self.auth.as_browser_cookie_for(domain)?;

        self.page
            .set_cookies(vec![cookie])
            .await
            .context("failed to set cookie")?;

        debug!(self.participant_config.username, "Set cookie");

        Ok(())
    }

    async fn join(&mut self) -> Result<()> {
        if self.state.borrow().joined {
            warn!("Already joined.");
            return Ok(());
        }

        // Create a new page if none exists
        self.set_cookie().await?;

        // -=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-
        // Navigate to the session URL
        self.page
            .goto(self.participant_config.session_url.to_string())
            .await
            .context("failed to wait for navigation response")?;

        debug!(self.participant_config.username, "Navigated to page");

        // -=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-
        // Find the input box to enter the name
        let input = wait_for_element(
            &self.page,
            r#"[data-testid="trigger-join-name"]"#,
            Duration::from_secs(30),
        )
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
            .type_str(&self.participant_config.username)
            .await
            .context("failed to insert name")?;

        debug!(self.participant_config.username, "Set the name of the participant");

        // -=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-
        // Find the join button
        wait_for_element(
            &self.page,
            r#"button[type="submit"]:not([disabled])"#,
            Duration::from_secs(30),
        )
        .await?;

        if let Err(err) = self.apply_all_settings().await {
            error!("Failed to apply settings before joining: {err}");
        }

        // -=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-
        // Click the join button
        self.find_element(r#"button[type="submit"]:not([disabled])"#)
            .await?
            .click()
            .await
            .context("failed to click join button")?;

        debug!(self.participant_config.username, "Clicked on the join button");

        // -=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-
        // Ensure we have joined the space.
        wait_for_element(
            &self.page,
            r#"[data-testid="trigger-leave-call"]"#,
            Duration::from_secs(30),
        )
        .await
        .context("We haven't joined the space, cannot find the leave button")?;

        info!(self.participant_config.username, "Joined the space");

        self.update_state().await;

        Ok(())
    }

    async fn apply_all_settings(&self) -> Result<()> {
        let Config {
            noise_suppression,
            transport,
            resolution,
            blur,
            audio_enabled,
            video_enabled,
            ..
        } = &self.participant_config.app_config;
        set_noise_suppression(&self.page, *noise_suppression)
            .await
            .context("failed to set noise suppression")?;
        set_background_blur(&self.page, *blur)
            .await
            .context("failed to set background blur")?;
        set_outgoing_camera_resolution(&self.page, resolution)
            .await
            .context("failed to set outgoing camera resolution")?;
        set_force_webrtc(&self.page, transport == &TransportMode::WebRTC)
            .await
            .context("failed to set transport mode")?;

        if !audio_enabled {
            self.toggle_audio().await?;
        }

        if !video_enabled {
            self.toggle_video().await?;
        }

        Ok(())
    }

    async fn leave(&self) -> Result<()> {
        self.leave_button()
            .await?
            .click()
            .await
            .context("Could not click on the leave space button")?;

        info!(self.participant_config.username, "Left the space");

        Ok(())
    }

    async fn close(self, mut browser: Browser) -> Result<()> {
        debug!(self.participant_config.username, "Closing the browser...");

        if let Err(err) = self.leave().await {
            error!(
                self.participant_config.username,
                "Failed leaving space while closing browser: {}", err
            );
        }

        if let Err(err) = self.page.close().await {
            error!("Error closing page: {err}");
        }

        browser.close().await?;
        browser.wait().await?;

        info!(self.participant_config.username, "Closed the browser");

        self.state.send_modify(|state| {
            state.running = false;
        });

        Ok(())
    }

    async fn toggle_audio(&self) -> Result<()> {
        self.mute_button()
            .await?
            .click()
            .await
            .context("Could not click on the toggle audio button")?;

        info!(self.participant_config.username, "Toggled audio");

        Ok(())
    }

    async fn toggle_video(&self) -> Result<()> {
        self.camera_button()
            .await?
            .click()
            .await
            .context("Could not click on the toggle camera button")?;

        info!(self.participant_config.username, "Toggled camera");

        Ok(())
    }

    async fn set_webcam_resolutions(&self, value: WebcamResolution) -> Result<()> {
        debug!(self.participant_config.username, "Changing to {value} resolution");

        set_outgoing_camera_resolution(&self.page, &value)
            .await
            .context("Failed to set outgoing camera resolution")?;

        Ok(())
    }

    async fn set_noise_suppression(&self, value: NoiseSuppression) -> Result<()> {
        info!(
            self.participant_config.username,
            "Changing noise suppression to {value}"
        );

        set_noise_suppression(&self.page, value)
            .await
            .context("Failed to set noise suppression level")?;

        self.update_state().await;

        Ok(())
    }

    async fn toggle_background_blur(&self) -> Result<()> {
        let background_blur = get_background_blur(&self.page).await?;
        set_background_blur(&self.page, !background_blur)
            .await
            .context("Failed to set background blur")?;
        self.update_state().await;
        Ok(())
    }
}

impl ParticipantInner {
    async fn leave_button(&self) -> Result<Element> {
        self.find_element(r#"button[data-testid="trigger-leave-call"]"#).await
    }

    async fn mute_button(&self) -> Result<Element> {
        self.find_element(r#"[data-testid="toggle-audio"]"#).await
    }

    async fn camera_button(&self) -> Result<Element> {
        self.find_element(r#"[data-testid="toggle-video"]"#).await
    }

    async fn find_element(&self, selector: &str) -> Result<Element> {
        self.page
            .find_element(selector)
            .await
            .context(format!("Could not find the {selector} element"))
    }

    async fn update_state(&self) {
        let joined = self.leave_button().await.is_ok();
        let mut muted = false;
        let mut video_activated = false;
        let mut transport_mode = TransportMode::default();
        let mut webcam_resolution = WebcamResolution::default();
        let mut noise_suppression = NoiseSuppression::default();
        let mut background_blur = false;

        if let Ok(value) = get_noise_suppression(&self.page).await {
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

        if let Ok(value) = get_force_webrtc(&self.page).await {
            if value {
                transport_mode = TransportMode::WebRTC;
            }
        }

        if let Ok(value) = get_outgoing_camera_resolution(&self.page).await {
            webcam_resolution = value
        }

        if let Ok(blur) = get_background_blur(&self.page).await {
            background_blur = blur;
        }

        self.state.send_modify(|state| {
            state.joined = joined;
            state.muted = muted;
            state.video_activated = video_activated;
            state.transport_mode = transport_mode;
            state.webcam_resolution = webcam_resolution;
            state.noise_suppression = noise_suppression;
            state.background_blur = background_blur;
        });
    }
}

async fn element_state(el: &Element) -> Option<bool> {
    el.attribute("data-test-state")
        .await
        .ok()
        .unwrap_or(None)
        .map(|v| v == "true")
}
