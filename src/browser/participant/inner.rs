use super::{
    commands::{
        get_noise_suppression_eval,
        set_noise_suppression_eval,
    },
    messages::ParticipantMessage,
    state::{
        NoiseSuppression,
        TransportMode,
        WebcamResolution,
    },
    ParticipantState,
};
use crate::{
    browser::{
        auth::{
            BorrowedCookie,
            HyperSessionCookieManger,
        },
        create_browser,
        participant::commands::get_background_blur_eval,
        wait_for_element,
    },
    config::{
        BrowserConfig,
        ParticipantConfig,
    },
};
use chromiumoxide::{
    cdp::browser_protocol::target::CreateTargetParams,
    Browser,
    Element,
    Handler,
    Page,
};
use eyre::{
    bail,
    eyre,
    Context as _,
    ContextCompat as _,
    OptionExt,
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
    page: Option<Page>,
    state: watch::Sender<ParticipantState>,
    auth: BorrowedCookie,
}

impl ParticipantInner {
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

        let (browser, handler) = create_browser(&BrowserConfig::from(&participant_config)).await?;

        let browser_event_task_handle = Self::drive_browser_events(&participant_config.username, handler);

        let participant = Self {
            participant_config,
            page: None,
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

    #[instrument(level = "debug", skip(self, browser, receiver), fields(name = %self.participant_config.username))]
    async fn handle_actions(
        mut self,
        mut browser: Browser,
        mut receiver: UnboundedReceiver<ParticipantMessage>,
    ) -> Result<()> {
        self.state.send_modify(|state| {
            state.running = true;
        });

        if let Err(err) = self.join(&mut browser).await {
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
                ParticipantMessage::Join => self.join(&mut browser).await,
                ParticipantMessage::Leave => self.leave().await,
                ParticipantMessage::Close => {
                    self.close(browser).await?;
                    return Ok(());
                }
                ParticipantMessage::ToggleAudio => self.toggle_audio().await,
                ParticipantMessage::ToggleVideo => self.toggle_video().await,
                ParticipantMessage::ToggleTransportMode => self.toggle_transport_mode().await,
                ParticipantMessage::ToggleThroughWebcamResolutions => self.toggle_through_webcam_resolutions().await,
                ParticipantMessage::ToggleNoiseSuppression => self.toggle_through_noise_suppression().await,
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
}

impl ParticipantInner {
    async fn set_cookie(&self, page: &Page) -> Result<()> {
        let domain = self.participant_config.session_url.host_str().unwrap_or("localhost");

        let cookie = self.auth.as_browser_cookie_for(domain)?;

        page.set_cookies(vec![cookie]).await.context("failed to set cookie")?;

        debug!(self.participant_config.username, "Set cookie");

        Ok(())
    }

    async fn create_page(&mut self, browser: &mut Browser) -> Result<Page> {
        let page = if let Ok(Some(page)) = browser
            .pages()
            .await
            .context("failed to get pages")
            .map(|pages| pages.into_iter().next())
        {
            page.goto(self.participant_config.session_url.to_string())
                .await
                .context("failed to navigate to session_url")?;
            page
        } else {
            browser
                .new_page(
                    CreateTargetParams::builder()
                        .url(self.participant_config.session_url.to_string())
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
                    self.participant_config.username, self.participant_config.session_url,
                )
            })?;

        if let Some(text) = &navigation.failure_text {
            bail!(
                "{}: When creating a new page request got a failure: {}",
                self.participant_config.username,
                text
            );
        }

        debug!(
            self.participant_config.username,
            "Created a new page for the {}", self.participant_config.session_url
        );

        Ok(page)
    }

    async fn open_debug_panel(&self) -> Result<()> {
        let debug_button = self
            .find_element(r#"button[data-testid="toggle-debug-panel"]"#)
            .await?
            .ok_or_eyre("Settings collapsible button not found")?;

        let state = element_state(&debug_button).await.unwrap_or_default();
        if !state {
            debug_button.click().await.context("Could not open the debug panel")?;
        }

        let settings_collapsible_button = self
            .find_element(r#"button[data-testid="toggle-debug-settings-collapsible"]"#)
            .await?
            .ok_or_eyre("Settings collapsible button not found")?;

        let state = element_state(&settings_collapsible_button).await.unwrap_or_default();
        if !state {
            settings_collapsible_button
                .click()
                .await
                .context("Could not open the settings collapsible")?;
        }

        debug!(self.participant_config.username, "Opened the debug settings panel");

        Ok(())
    }

    async fn join(&mut self, browser: &mut Browser) -> Result<()> {
        if self.state.borrow().joined {
            warn!("Already joined.");
            return Ok(());
        }

        // Create a new page if none exists
        if self.page.is_none() {
            let mut backoff = hyper_video_maybe_backoff::MaybeBackoff::default();
            let mut attempt = 0;
            loop {
                backoff.sleep().await;
                match self.create_page(browser).await {
                    Ok(page) => {
                        self.page = Some(page);
                        break;
                    }
                    Err(_) if attempt < 5 => {
                        attempt += 1;
                        backoff.arm();
                        warn!(?attempt, "Failed to create a new page, retrying...");
                    }
                    Err(err) => return Err(err),
                }
            }
        }

        let page = self
            .page
            .as_ref()
            .ok_or_eyre("Unexpectedly, there was no page when joining.")?;
        self.set_cookie(page).await?;

        // Navigate and interact (similar to WebBrowser)
        page.goto(self.participant_config.session_url.to_string())
            .await
            .context("failed to wait for navigation response")?;

        debug!(self.participant_config.username, "Navigated to page");

        // Find the input box to enter the name
        let input = wait_for_element(page, r#"[data-testid="trigger-join-name"]"#, Duration::from_secs(30))
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

        // Find the join button and click it
        wait_for_element(
            page,
            r#"button[type="submit"]:not([disabled])"#,
            Duration::from_secs(30),
        )
        .await?
        .click()
        .await
        .context("failed to click join button")?;

        debug!(self.participant_config.username, "Clicked on the join button");

        // Ensure we have joined the space.
        wait_for_element(page, r#"[data-testid="trigger-leave-call"]"#, Duration::from_secs(30))
            .await
            .context("We haven't joined the space, cannot find the leave button")?;

        info!(self.participant_config.username, "Joined the space");

        self.open_debug_panel().await?;

        self.update_state().await;

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

        if let Some(page) = self.page {
            page.close().await.context("failed to close browser")?;
        }

        browser.close().await?;
        browser.wait().await?;

        info!(self.participant_config.username, "Closed the browser");

        self.state.send_modify(|state| {
            state.running = false;
        });

        Ok(())
    }

    pub async fn toggle_audio(&self) -> Result<()> {
        self.mute_button()
            .await?
            .click()
            .await
            .context("Could not click on the toggle audio button")?;

        info!(self.participant_config.username, "Toggled audio");

        Ok(())
    }

    pub async fn toggle_video(&self) -> Result<()> {
        self.camera_button()
            .await?
            .click()
            .await
            .context("Could not click on the toggle camera button")?;

        info!(self.participant_config.username, "Toggled camera");

        Ok(())
    }

    pub async fn toggle_transport_mode(&self) -> Result<()> {
        self.open_debug_panel().await?;
        self.webrtc_checkbox()
            .await?
            .click()
            .await
            .context("Could not click on the WebRTC checkbox")?;

        debug!(self.participant_config.username, "Toggled transport mode");

        // Reload the page to apply the changes
        let page = self.page.as_ref().ok_or_eyre("Page not found")?;
        page.reload().await.context("Could not reload the page")?;
        page.wait_for_navigation()
            .await
            .context("Failed to wait for navigation")?;

        debug!(self.participant_config.username, "Page reloaded");

        // Find the join button and click it
        wait_for_element(
            page,
            r#"button[type="submit"]:not([disabled])"#,
            Duration::from_secs(30),
        )
        .await?
        .click()
        .await
        .context("failed to click submit button")?;

        debug!(self.participant_config.username, "Joining the space again");

        tokio::time::sleep(Duration::from_secs(2)).await;

        self.update_state().await;

        info!(self.participant_config.username, "Toggled transport mode");

        Ok(())
    }

    pub async fn toggle_through_webcam_resolutions(&self) -> Result<()> {
        self.open_debug_panel().await?;

        let resolution = self.state.borrow().webcam_resolution.next();

        let select = self.resolution_select_button().await?;

        debug!(self.participant_config.username, "Changing to {resolution} resolution");

        select
            .click()
            .await
            .context("Could not click on the resolution select")?;

        let selector = format!("div[data-test-value=\"{resolution}\"]");
        let option = self
            .find_element(&selector)
            .await?
            .ok_or_else(|| eyre!("Resolution option not found"))?;

        option
            .click()
            .await
            .context("Could not click on the resolution option")?;

        info!(self.participant_config.username, "Changed to {resolution} resolution");

        Ok(())
    }

    pub async fn toggle_through_noise_suppression(&self) -> Result<()> {
        if let Some(page) = &self.page {
            let noise_suppression = self.read_noise_suppression_level().await?.next();

            set_noise_suppression_eval(page, noise_suppression.clone())
                .await
                .context("Failed to set noise suppression level")?;

            info!(
                self.participant_config.username,
                "Changed noise suppression to {noise_suppression}"
            );
        } else {
            error!("No page found to change noise suppression");
        }

        self.update_state().await;

        Ok(())
    }

    pub async fn toggle_background_blur(&self) -> Result<()> {
        if let Some(page) = &self.page {
            let background_blur = self.read_background_blur().await?;

            let new_state = !background_blur;

            page.evaluate(format!(
                "window.setBackgroundBlur({})",
                new_state.to_string().to_lowercase()
            ))
            .await
            .context("Failed to set background blur")?;

            info!(
                self.participant_config.username,
                "Changed background blur to {}", new_state
            );
        } else {
            error!("No page found to change background blur");
        }

        self.update_state().await;

        Ok(())
    }

    async fn read_noise_suppression_level(&self) -> Result<NoiseSuppression> {
        if let Some(page) = &self.page {
            return get_noise_suppression_eval(page).await;
        }
        Ok(NoiseSuppression::default())
    }

    async fn read_background_blur(&self) -> Result<bool> {
        if let Some(page) = &self.page {
            return get_background_blur_eval(page).await;
        }

        Ok(false)
    }
}

impl ParticipantInner {
    async fn leave_button(&self) -> Result<Element> {
        self.find_element(r#"button[data-testid="trigger-leave-call"]"#)
            .await?
            .ok_or_eyre("Leave not found")
    }

    async fn mute_button(&self) -> Result<Element> {
        self.find_element(r#"button[data-testid="toggle-audio"]"#)
            .await?
            .ok_or_eyre("Mute button not found")
    }

    async fn camera_button(&self) -> Result<Element> {
        self.find_element(r#"div[data-testid="toggle-camera"]"#)
            .await?
            .ok_or_eyre("Camera button not found")
    }

    async fn webrtc_checkbox(&self) -> Result<Element> {
        self.find_element(r#"button[data-testid="toggle-debug-force-webrtc"]"#)
            .await?
            .ok_or_eyre("WebRTC select not found")
    }

    async fn resolution_select_button(&self) -> Result<Element> {
        self.find_element(r#"div[data-testid="debug-webcam-resolution-camera"] button"#)
            .await?
            .ok_or_eyre("Resolution select not found")
    }

    async fn resolution_select_div(&self) -> Result<Element> {
        self.find_element(r#"div[data-testid="debug-webcam-resolution-camera"]"#)
            .await?
            .ok_or_eyre("Resolution select not found")
    }

    async fn find_element(&self, selector: &str) -> Result<Option<Element>> {
        if let Some(page) = self.page.as_ref() {
            let button = page
                .find_element(selector)
                .await
                .context(format!("Could not find the {selector} element"))?;

            return Ok(Some(button));
        }

        Ok(None)
    }

    async fn update_state(&self) {
        let mut joined = false;
        let mut muted = false;
        let mut video_activated = false;
        let mut transport_mode = TransportMode::default();
        let mut webcam_resolution = WebcamResolution::default();
        let mut noise_suppression = NoiseSuppression::default();
        let mut background_blur = false;

        if self.page.is_some() {
            joined = self.leave_button().await.is_ok();

            if let Ok(noise_suppression_level) = self.read_noise_suppression_level().await {
                noise_suppression = noise_suppression_level;
            }

            if let Err(err) = self.open_debug_panel().await {
                error!("Error getting the state, failed opening settings: {}", err);
                return;
            }

            if let Ok(mute_button) = self.mute_button().await {
                if let Some(element_state) = element_state(&mute_button).await {
                    muted = !element_state;
                }
            }

            if let Ok(camera_button) = self.camera_button().await {
                if let Some(element_state) = element_state(&camera_button).await {
                    video_activated = element_state;
                }
            }

            if let Ok(webrtc_checkbox) = self.webrtc_checkbox().await {
                let state = webrtc_checkbox.attribute("data-state").await.unwrap_or_default();
                if state == Some("checked".to_string()) {
                    transport_mode = TransportMode::WebRTC;
                }
            }

            if let Ok(resolution_select) = self.resolution_select_div().await {
                if let Some(resolution) = resolution_select.attribute("data-state").await.unwrap_or_default() {
                    webcam_resolution = WebcamResolution::from(resolution);
                }
            }

            if let Ok(blur) = self.read_background_blur().await {
                background_blur = blur;
            }
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
