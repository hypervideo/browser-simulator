use super::{
    messages::ParticipantMessage,
    ParticipantState,
};
use crate::{
    create_browser,
    participant::{
        messages::ParticipantLogMessage,
        selectors::lite,
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
        mpsc::{
            UnboundedReceiver,
            UnboundedSender,
        },
        watch,
    },
    task::JoinHandle,
};

#[derive(Debug)]
pub(super) struct ParticipantInnerLite {
    participant_config: ParticipantConfig,
    page: Page,
    state: watch::Sender<ParticipantState>,
    sender: UnboundedSender<ParticipantLogMessage>,
}

impl ParticipantInnerLite {
    #[instrument(level = "debug", skip_all, fields(name = %participant_config.username))]
    pub(super) async fn run(
        participant_config: ParticipantConfig,
        receiver: UnboundedReceiver<ParticipantMessage>,
        sender: UnboundedSender<ParticipantLogMessage>,
        state: watch::Sender<ParticipantState>,
    ) -> Result<()> {
        let (mut browser, handler) = create_browser(&BrowserConfig::from(&participant_config)).await?;
        let browser_event_task_handle = Self::drive_browser_events(&participant_config.username, handler);
        let page = Self::create_page_retry(&participant_config, &mut browser).await?;

        state.send_modify(|state| {
            state.username = participant_config.username.clone();
        });

        let participant = Self {
            participant_config,
            page,
            state: state.clone(),
            sender,
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
            self.send_log_message(
                "error",
                format!("Failed joining the session when starting the browser {err}"),
            );

            match browser.kill().await {
                Some(Ok(_)) => {
                    debug!("browser killed");
                    self.send_log_message("debug", "browser killed");
                }
                Some(Err(err)) => {
                    error!("failed to kill browser: {err}");
                    self.send_log_message("error", format!("failed to kill browser: {err}"));
                }
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

                Some(_) = detached_event.next() => {
                    warn!(self.participant_config.username, "Browser unexpectedly closed");
                    match browser.kill().await {
                        Some(Ok(_)) => {
                            debug!("browser killed");
                            self.send_log_message("debug", "browser killed");
                        },
                        Some(Err(err)) => {
                            error!("failed to kill browser: {err}");
                            self.send_log_message("error", format!("failed to kill browser: {err}"));
                        },
                        None => {
                            debug!("browser process not found");
                            self.send_log_message("debug", "browser process not found");
                        },
                    }
                    break;
                },

                Some(message) = receiver.recv() => { message }
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
                ParticipantMessage::ToggleScreenshare => self.toggle_screen_share().await,
                ParticipantMessage::SetWebcamResolutions(value) => self.set_webcam_resolutions(value).await,
                ParticipantMessage::SetNoiseSuppression(value) => self.set_noise_suppression(value).await,
                ParticipantMessage::ToggleBackgroundBlur => self.toggle_background_blur().await,
            } {
                error!("Running action {message} failed with error: {e}.");
                self.send_log_message("error", format!("Running action {message} failed with error: {e}."));
            }

            self.update_state().await;
        }

        self.state.send_modify(|state| {
            state.running = false;
        });

        Ok(())
    }

    fn send_log_message(&self, level: &str, message: impl ToString) {
        if let Err(err) = self.sender.send(ParticipantLogMessage::new(
            level,
            &self.participant_config.username,
            message,
        )) {
            trace!("Failed to send log message: {err}");
        }
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
        let mut backoff = maybe_backoff::MaybeBackoff::default();
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

impl ParticipantInnerLite {
    async fn join(&mut self) -> Result<()> {
        if self.state.borrow().joined {
            warn!("Already joined.");
            self.send_log_message("warn", "Already joined.");
            return Ok(());
        }

        // Navigate to session URL directly, lite auth is handled by frontend routing
        self.page
            .goto(self.participant_config.session_url.to_string())
            .await
            .context("failed to wait for navigation response")?;

        debug!(self.participant_config.username, "Navigated to page");
        self.send_log_message("debug", "Navigated to page");

        // Ensure join button exists
        wait_for_element(&self.page, lite::JOIN_BUTTON, Duration::from_secs(30)).await?;

        // Lite frontend doesn't support pre-join settings configuration

        // Click the join button
        self.find_element(lite::JOIN_BUTTON)
            .await?
            .click()
            .await
            .context("failed to click join button")?;

        debug!(self.participant_config.username, "Clicked on the join button");
        self.send_log_message("debug", "Clicked on the join button");

        // Ensure we have joined the space.
        wait_for_element(&self.page, lite::LEAVE_BUTTON, Duration::from_secs(30))
            .await
            .context("We haven't joined the space, cannot find the leave button")?;

        info!(self.participant_config.username, "Joined the space");
        self.send_log_message("info", "Joined the space");

        // Apply settings after joining
        if let Err(err) = self.apply_all_settings(false).await {
            error!("Failed to apply settings after joining: {err}");
            self.send_log_message("error", format!("Failed to apply settings after joining: {err}"));
        }

        self.update_state().await;
        Ok(())
    }

    async fn apply_all_settings(&self, _in_lobby: bool) -> Result<()> {
        let Config {
            audio_enabled,
            video_enabled,
            screenshare_enabled,
            ..
        } = &self.participant_config.app_config;

        // Only apply core settings that are supported in lite frontend
        if !audio_enabled {
            self.toggle_audio().await?;
        }

        if !video_enabled {
            self.toggle_video().await?;
        }

        if *screenshare_enabled {
            self.toggle_screen_share().await?;
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
        self.send_log_message("info", "Left the space");
        Ok(())
    }

    async fn close(self, mut browser: Browser) -> Result<()> {
        debug!(self.participant_config.username, "Closing the browser...");
        let _ = self.leave().await;
        let _ = self.page.clone().close().await;
        browser.close().await?;
        browser.wait().await?;
        info!(self.participant_config.username, "Closed the browser");
        self.send_log_message("info", "Closed the browser");
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
        self.send_log_message("info", "Toggled audio");
        self.update_state().await;
        Ok(())
    }

    async fn toggle_video(&self) -> Result<()> {
        self.camera_button()
            .await?
            .click()
            .await
            .context("Could not click on the toggle camera button")?;
        info!(self.participant_config.username, "Toggled camera");
        self.send_log_message("info", "Toggled camera");
        self.update_state().await;
        Ok(())
    }

    async fn toggle_screen_share(&self) -> Result<()> {
        self.screen_share_button()
            .await?
            .click()
            .await
            .context("Could not click on the toggle screen share button")?;
        info!(self.participant_config.username, "Toggled screen share");
        self.send_log_message("info", "Toggled screen share");
        // Give it some time to start share before updating state
        tokio::time::sleep(Duration::from_secs(1)).await;
        self.update_state().await;
        Ok(())
    }

    async fn set_webcam_resolutions(&self, _value: WebcamResolution) -> Result<()> {
        // Lite frontend doesn't support webcam resolution changes
        debug!(
            self.participant_config.username,
            "Webcam resolution changes not supported in lite frontend"
        );
        Ok(())
    }

    async fn set_noise_suppression(&self, _value: NoiseSuppression) -> Result<()> {
        // Lite frontend doesn't support noise suppression changes
        debug!(
            self.participant_config.username,
            "Noise suppression changes not supported in lite frontend"
        );
        Ok(())
    }

    async fn toggle_background_blur(&self) -> Result<()> {
        // Lite frontend doesn't support background blur changes
        debug!(
            self.participant_config.username,
            "Background blur changes not supported in lite frontend"
        );
        Ok(())
    }
}

impl ParticipantInnerLite {
    async fn leave_button(&self) -> Result<Element> {
        self.find_element(lite::LEAVE_BUTTON).await
    }
    async fn mute_button(&self) -> Result<Element> {
        self.find_element(lite::MUTE_BUTTON).await
    }
    async fn camera_button(&self) -> Result<Element> {
        self.find_element(lite::VIDEO_BUTTON).await
    }
    async fn screen_share_button(&self) -> Result<Element> {
        self.find_element(lite::SCREEN_SHARE_BUTTON).await
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
        let mut screenshare_activated = false;

        // Only check core operations that are supported in lite frontend
        if let Ok(mute_button) = self.mute_button().await {
            if let Some(value) = element_state(&mute_button).await {
                muted = !value;
            }
        }
        if let Ok(camera_button) = self.camera_button().await {
            if let Some(v) = element_state(&camera_button).await {
                video_activated = v;
            }
        }
        if let Ok(screen_share_button) = self.screen_share_button().await {
            debug!(
                self.participant_config.username,
                "Screen share button: {screen_share_button:?}"
            );
            if let Some(v) = element_state(&screen_share_button).await {
                screenshare_activated = v;
            }
        }

        self.state.send_modify(|state| {
            state.joined = joined;
            state.muted = muted;
            state.video_activated = video_activated;
            state.screenshare_activated = screenshare_activated;
            // Set defaults for unsupported features
            state.transport_mode = TransportMode::default();
            state.webcam_resolution = WebcamResolution::default();
            state.noise_suppression = NoiseSuppression::default();
            state.background_blur = false;
            debug!("Sending state update: {state:?}");
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
