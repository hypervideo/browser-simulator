use super::{
    auth::{
        BorrowedCookie,
        HyperSessionCookieManger,
        HyperSessionCookieStash,
    },
    create_browser,
    wait_for_element,
};
use crate::config::{
    BrowserConfig,
    Config,
    ParticipantConfig,
};
use chromiumoxide::{
    browser::Browser,
    cdp::browser_protocol::target::CreateTargetParams,
    Element,
    Handler,
    Page,
};
use derive_more::Display;
use eyre::{
    bail,
    Context as _,
    ContextCompat as _,
    OptionExt,
    Result,
};
use futures::StreamExt as _;
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        Arc,
        Mutex,
    },
    time::Duration,
    vec::IntoIter,
};
use tokio::{
    sync::{
        mpsc::{
            unbounded_channel,
            UnboundedReceiver,
            UnboundedSender,
        },
        watch,
    },
    task::JoinHandle,
};
use tokio_util::sync::{
    CancellationToken,
    DropGuard,
};

#[derive(Display)]
pub enum ParticipantMessage {
    Join,
    Leave,
    Close,
    ToggleAudio,
    ToggleVideo,
}

/// Store for all the participants that we will expose to the TUI
/// for displaying and control.
#[derive(Debug, Clone)]
pub struct ParticipantStore {
    cookies: HyperSessionCookieManger,
    inner: Arc<Mutex<HashMap<String, Participant>>>,
}

impl ParticipantStore {
    pub(crate) fn new(data_dir: impl AsRef<Path>) -> Self {
        Self {
            cookies: HyperSessionCookieStash::load_from_data_dir(data_dir).into(),
            inner: Default::default(),
        }
    }

    pub(crate) fn spawn(&self, config: &Config) -> Result<()> {
        let participant = Participant::spawn_with_app_config(config, self.cookies.clone())?;
        self.add(participant);
        Ok(())
    }

    pub(crate) fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    fn sorted(&self) -> IntoIter<Participant> {
        let mut participants = self.inner.lock().unwrap().values().cloned().collect::<Vec<_>>();
        participants.sort_by(|a, b| a.created.cmp(&b.created));

        participants.into_iter()
    }

    pub(crate) fn keys(&self) -> Vec<String> {
        self.sorted().map(|p| p.name.clone()).collect()
    }

    pub(crate) fn values(&self) -> Vec<Participant> {
        self.sorted().collect()
    }

    pub(crate) fn add(&self, participant: Participant) {
        self.inner.lock().unwrap().insert(participant.name.clone(), participant);
    }

    pub(crate) fn remove(&self, name: &str) -> Option<Participant> {
        self.inner.lock().unwrap().remove(name)
    }

    pub(crate) fn get(&self, name: &str) -> Option<Participant> {
        self.inner.lock().unwrap().get(name).cloned()
    }

    pub(crate) fn prev(&self, name: &str) -> Option<String> {
        let sorted = self.sorted().collect::<Vec<_>>();
        let index = sorted.iter().position(|p| p.name == name)?;
        (index > 0).then(|| sorted[index - 1].name.clone())
    }
}

/// Participant that will spawn a browser and join the given space from the config.
#[derive(Debug, Clone)]
pub struct Participant {
    pub name: String,
    pub created: chrono::DateTime<chrono::Utc>,
    pub state: watch::Receiver<ParticipantState>,
    _participant_task_guard: Arc<DropGuard>,
    sender: UnboundedSender<ParticipantMessage>,
}

#[derive(Debug, Default, Clone)]
pub struct ParticipantState {
    pub running: bool,
    pub joined: bool,
    pub muted: bool,
    pub video_activated: bool,
}

impl PartialEq for Participant {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Participant {
    pub fn spawn_with_app_config(config: &Config, cookie_manager: HyperSessionCookieManger) -> Result<Self> {
        let session_url = url::Url::parse(&config.url).context("failed to parse url")?;
        let base_url = session_url.origin().unicode_serialization();
        let cookie = cookie_manager.give_cookie(&base_url);
        let name = cookie.as_ref().map(|c| c.username());
        let participant_config = ParticipantConfig::new(config, name)?;
        Self::with_participant_config(participant_config, cookie, cookie_manager)
    }

    pub fn with_participant_config(
        participant_config: ParticipantConfig,
        cookie: Option<BorrowedCookie>,
        cookie_manager: HyperSessionCookieManger,
    ) -> Result<Self> {
        let (sender, receiver) = unbounded_channel::<ParticipantMessage>();

        let name = participant_config.username.clone();
        let task_cancellation_token = CancellationToken::new();
        let task_cancellation_guard = task_cancellation_token.clone().drop_guard();
        let (state_sender, state_receiver) = watch::channel(Default::default());

        tokio::task::spawn({
            let name = name.clone();
            async move {
                tokio::select! {
                    biased;
                    _ = task_cancellation_token.cancelled() => {},

                    result = ParticipantInner::run(
                        participant_config,
                        cookie,
                        cookie_manager,
                        receiver,
                        state_sender,
                    ) => {
                        if let Err(err) = result {
                            error!(?name, "Failed to create participant: {err}")
                        }
                    }
                };

                debug!(?name, "Participant task canceled");
            }
        });

        Ok(Self {
            name,
            created: chrono::Utc::now(),
            state: state_receiver,
            _participant_task_guard: Arc::new(task_cancellation_guard),
            sender,
        })
    }

    pub fn join(&self) {
        let state = self.state.borrow();
        if !state.running {
            debug!(self.name, "Already closed the browser");
            return;
        }
        if state.joined {
            debug!(self.name, "Already joined");
            return;
        }
        self.sender
            .send(ParticipantMessage::Join)
            .expect("Was not able to send ParticipantMessage::Join message")
    }

    pub fn leave(&self) {
        let state = self.state.borrow();
        if !state.running {
            debug!(self.name, "Already closed the browser");
            return;
        }
        if !state.joined {
            debug!(self.name, "Not in the space yet");
            return;
        }
        self.sender
            .send(ParticipantMessage::Leave)
            .expect("Was not able to send ParticipantMessage::Leave message");
    }

    pub async fn close(mut self) {
        if !self.state.borrow().running {
            debug!(self.name, "Already closed the browser");
            return;
        }

        self.sender
            .send(ParticipantMessage::Close)
            .expect("Was not able to send ParticipantMessage::Close message");

        if let Err(err) = self.state.wait_for(|state| !state.running).await {
            error!("Failed to wait for participant to close: {err}");
        };
    }

    pub fn toggle_audio(&self) {
        let state = self.state.borrow();
        if !state.running {
            debug!(self.name, "Already closed the browser");
            return;
        }
        if !state.joined {
            debug!(self.name, "Cannot toggle audio, not in the space yet");
            return;
        }
        self.sender
            .send(ParticipantMessage::ToggleAudio)
            .expect("Was not able to send ParticipantMessage::ToggleAudio message")
    }

    pub fn toggle_video(&self) {
        let state = self.state.borrow();
        if !state.running {
            debug!(self.name, "Already closed the browser");
            return;
        }
        if !state.joined {
            debug!(self.name, "Cannot toggle video, not in the space yet");
            return;
        }
        self.sender
            .send(ParticipantMessage::ToggleVideo)
            .expect("Was not able to send ParticipantMessage::ToggleVideo message")
    }
}

/// Async participant "worker" that holds the browser and session.
/// It has the direct command to the browser session that can modify the
/// participant behavior in the space.
/// It holds the message receiver that is used to handle the incoming messages
/// from the sync TUI runtime.
#[derive(Debug)]
struct ParticipantInner {
    participant_config: ParticipantConfig,
    page: Option<Page>,
    state: watch::Sender<ParticipantState>,
    auth: BorrowedCookie,
}

impl ParticipantInner {
    async fn run(
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

    async fn set_cookie(&self, page: &Page) -> Result<()> {
        let domain = self.participant_config.session_url.host_str().unwrap_or("localhost");
        let cookie = self.auth.as_browser_cookie_for(domain)?;

        page.set_cookies(vec![cookie]).await.context("failed to set cookie")?;

        debug!(self.participant_config.username, "Set cookie");

        Ok(())
    }

    async fn ensure_page_exists(&mut self, browser: &mut Browser) -> Result<()> {
        if self.page.is_none() {
            let page = browser
                .new_page(
                    CreateTargetParams::builder()
                        .url(self.participant_config.session_url.to_string())
                        .build()
                        .map_err(|e| eyre::eyre!(e))?,
                )
                .await
                .context("failed to create new page")?;

            let arc_request = page
                .wait_for_navigation_response()
                .await
                .context("Page could not navigate to session_url")?
                .with_context(|| {
                    format!(
                        "{}: No request returned when creating a page for {}",
                        self.participant_config.username, self.participant_config.session_url,
                    )
                })?;

            if let Some(text) = &arc_request.failure_text {
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

            self.page = Some(page);
        }

        Ok(())
    }

    async fn join(&mut self, browser: &mut Browser) -> Result<()> {
        if self.state.borrow().joined {
            warn!("Already joined.");
            return Ok(());
        }

        // Create a new page if none exists
        self.ensure_page_exists(browser).await?;
        let page = match &self.page {
            Some(page) => page,
            None => bail!("Unexpectedly, there was no page when joining."),
        };

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
        .context("failed to click submit button")?;

        debug!(self.participant_config.username, "Clicked on the join button");

        // Ensure we have joined the space.
        wait_for_element(page, r#"[data-testid="trigger-leave-call"]"#, Duration::from_secs(30))
            .await
            .context("We haven't joined the space, cannot find the leave button")?;

        info!(self.participant_config.username, "Joined the space");

        self.state.send_modify(|state| {
            state.joined = true;
            state.muted = false;
            state.video_activated = true;
        });

        Ok(())
    }

    async fn leave(&self) -> Result<()> {
        info!(self.participant_config.username, "Leaving the space...");

        let leave_button = self.leave_button().await?;
        debug!("Clicking on the leave space button");

        leave_button
            .click()
            .await
            .context("Could not click on the leave space button")?;

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

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

        Ok(())
    }

    pub async fn toggle_audio(&self) -> Result<()> {
        let mute_button = self.mute_button().await?;
        mute_button
            .click()
            .await
            .context("Could not click on the toggle audio button")?;

        info!(self.participant_config.username, "Toggled audio");

        Ok(())
    }

    pub async fn toggle_video(&self) -> Result<()> {
        let camera_button = self.camera_button().await?;

        camera_button
            .click()
            .await
            .context("Could not click on the toggle camera button")?;

        info!(self.participant_config.username, "Toggled camera");

        Ok(())
    }

    async fn leave_button(&self) -> Result<Element> {
        self.get_button(r#"button[data-testid="trigger-leave-call"]"#)
            .await?
            .ok_or_eyre("Leave not found")
    }

    async fn mute_button(&self) -> Result<Element> {
        self.get_button(r#"button[data-testid="toggle-audio"]"#)
            .await?
            .ok_or_eyre("Mute button not found")
    }

    async fn camera_button(&self) -> Result<Element> {
        self.get_button(r#"div[data-testid="toggle-camera"]"#)
            .await?
            .ok_or_eyre("Camera button not found")
    }

    async fn get_button(&self, selector: &str) -> Result<Option<Element>> {
        if let Some(page) = self.page.as_ref() {
            let button = page
                .find_element(selector)
                .await
                .context(format!("Could not find the {selector} button"))?;

            return Ok(Some(button));
        }

        Ok(None)
    }

    async fn update_state(&self) {
        let mut joined = false;
        let mut muted = false;
        let mut video_activated = false;

        if self.page.is_some() {
            joined = self.leave_button().await.is_ok();

            if let Ok(mute_button) = self.mute_button().await {
                if let Some(button_state) = button_state(mute_button).await {
                    muted = !button_state;
                }
            }

            if let Ok(camera_button) = self.camera_button().await {
                if let Some(button_state) = button_state(camera_button).await {
                    video_activated = button_state;
                }
            }
        }

        self.state.send_modify(|state| {
            state.joined = joined;
            state.muted = muted;
            state.video_activated = video_activated;
        });
    }
}

async fn button_state(el: Element) -> Option<bool> {
    el.attribute("data-test-state")
        .await
        .ok()
        .unwrap_or(None)
        .map(|v| v == "true")
}
