use super::{
    auth::AuthToken,
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
    Page,
};
use eyre::{
    Context as _,
    Result,
};
use futures::StreamExt as _;
use std::{
    collections::HashMap,
    sync::{
        atomic::{
            AtomicBool,
            Ordering::Relaxed,
        },
        Arc,
        Mutex,
    },
    time::Duration,
    vec::IntoIter,
};
use tokio::{
    sync::mpsc::{
        unbounded_channel,
        UnboundedReceiver,
        UnboundedSender,
    },
    task::JoinHandle,
};

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
    inner: Arc<Mutex<HashMap<String, Participant>>>,
}

impl ParticipantStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn spawn(&self, config: &Config) -> Result<()> {
        let participant = Participant::new(config)?;
        self.add(participant);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    fn sorted(&self) -> IntoIter<Participant> {
        let mut participants = self.inner.lock().unwrap().values().cloned().collect::<Vec<_>>();
        participants.sort_by(|a, b| a.created.cmp(&b.created));

        participants.into_iter()
    }

    pub fn keys(&self) -> Vec<String> {
        self.sorted().map(|p| p.name.clone()).collect()
    }

    pub fn values(&self) -> Vec<Participant> {
        self.sorted().collect()
    }

    pub fn add(&self, participant: Participant) {
        self.inner.lock().unwrap().insert(participant.name.clone(), participant);
    }

    pub fn remove(&self, name: &str) -> Option<Participant> {
        self.inner.lock().unwrap().remove(name)
    }

    pub fn get(&self, name: &str) -> Option<Participant> {
        self.inner.lock().unwrap().get(name).cloned()
    }
}

/// Participant that will spawn a browser and join the given space from the config.
#[derive(Debug, Clone)]
pub struct Participant {
    pub name: String,
    pub created: chrono::DateTime<chrono::Utc>,
    pub running: Arc<AtomicBool>,
    pub joined: Arc<AtomicBool>,
    pub muted: Arc<AtomicBool>,
    pub invisible: Arc<AtomicBool>,
    _handle: Arc<JoinHandle<()>>,
    sender: UnboundedSender<ParticipantMessage>,
}

impl PartialEq for Participant {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Participant {
    pub fn new(config: &Config) -> Result<Self> {
        let (sender, receiver) = unbounded_channel::<ParticipantMessage>();
        let participant_config = ParticipantConfig::new(config)?;
        let name = participant_config.name.clone();
        let name_cloned = name.clone();
        let running = Arc::new(AtomicBool::new(true));
        let running_cloned = running.clone();
        let joined = Arc::new(AtomicBool::new(false));
        let joined_cloned = joined.clone();
        let muted = Arc::new(AtomicBool::new(false));
        let muted_cloned = muted.clone();
        let invisible = Arc::new(AtomicBool::new(false));
        let invisible_cloned = invisible.clone();

        let handle = tokio::task::spawn(async move {
            if let Err(err) = ParticipantInner::spawn(
                participant_config,
                receiver,
                running_cloned,
                joined_cloned,
                muted_cloned,
                invisible_cloned,
            )
            .await
            {
                error!(name_cloned, "Failed to create participant: {}", err)
            }
        });

        Ok(Self {
            name,
            created: chrono::Utc::now(),
            running,
            joined,
            muted,
            invisible,
            _handle: Arc::new(handle),
            sender,
        })
    }

    pub fn join(&self) {
        if !self.running.load(Relaxed) {
            debug!(self.name, "Already closed the browser");
            return;
        }
        if self.joined.load(Relaxed) {
            debug!(self.name, "Already joined");
            return;
        }
        self.sender
            .send(ParticipantMessage::Join)
            .expect("Was not able to send ParticipantMessage::Join message")
    }

    pub fn leave(&self) {
        if !self.running.load(Relaxed) {
            debug!(self.name, "Already closed the browser");
            return;
        }
        if !self.joined.load(Relaxed) {
            debug!(self.name, "Not in the space yet");
            return;
        }
        self.sender
            .send(ParticipantMessage::Leave)
            .expect("Was not able to send ParticipantMessage::Leave message")
    }

    pub fn close(self) {
        if !self.running.load(Relaxed) {
            debug!(self.name, "Already closed the browser");
            return;
        }
        self.sender
            .send(ParticipantMessage::Close)
            .expect("Was not able to send ParticipantMessage::Close message")
    }

    pub fn toggle_audio(&self) {
        if !self.running.load(Relaxed) {
            debug!(self.name, "Already closed the browser");
            return;
        }
        if !self.joined.load(Relaxed) {
            debug!(self.name, "Cannot toggle audio, not in the space yet");
            return;
        }
        self.sender
            .send(ParticipantMessage::ToggleAudio)
            .expect("Was not able to send ParticipantMessage::ToggleAudio message")
    }

    pub fn toggle_video(&self) {
        if !self.running.load(Relaxed) {
            debug!(self.name, "Already closed the browser");
            return;
        }
        if !self.joined.load(Relaxed) {
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
    browser: Browser,
    page: Option<Page>,
    handle: JoinHandle<()>,
    receiver: UnboundedReceiver<ParticipantMessage>,
    running: Arc<AtomicBool>,
    joined: Arc<AtomicBool>,
    muted: Arc<AtomicBool>,
    invisible: Arc<AtomicBool>,
    auth: AuthToken,
}

impl ParticipantInner {
    async fn new(
        participant_config: ParticipantConfig,
        receiver: UnboundedReceiver<ParticipantMessage>,
        running: Arc<AtomicBool>,
        joined: Arc<AtomicBool>,
        muted: Arc<AtomicBool>,
        invisible: Arc<AtomicBool>,
    ) -> Result<Self> {
        let (browser, mut handler) = create_browser(&BrowserConfig::from(&participant_config)).await?;
        let name = participant_config.name.clone();

        let handle = tokio::task::spawn(async move {
            while let Some(event) = handler.next().await {
                if let Err(err) = event {
                    error!(name, "error in browser handler: {err:?}");
                }
            }
        });

        let base_url = participant_config.url.origin().unicode_serialization();
        let mut auth = AuthToken::fetch_token(&base_url).await?;
        auth.set_name(&participant_config.name, &base_url).await?;

        Ok(Self {
            participant_config,
            browser,
            page: None,
            handle,
            receiver,
            running,
            joined,
            muted,
            invisible,
            auth,
        })
    }

    async fn spawn(
        participant_config: ParticipantConfig,
        receiver: UnboundedReceiver<ParticipantMessage>,
        running: Arc<AtomicBool>,
        joined: Arc<AtomicBool>,
        muted: Arc<AtomicBool>,
        invisible: Arc<AtomicBool>,
    ) -> Result<()> {
        let running_clone = running.clone();

        let participant = Self::new(participant_config, receiver, running, joined, muted, invisible)
            .await
            .inspect_err(|_| {
                running_clone.store(false, Relaxed);
            })?;
        participant.handle_actions().await.inspect_err(|_| {
            running_clone.store(false, Relaxed);
        })?;

        Ok(())
    }

    async fn handle_actions(mut self) -> Result<()> {
        self.join().await?;

        while let Some(message) = self.receiver.recv().await {
            match message {
                ParticipantMessage::Join => {
                    self.join().await?;
                }
                ParticipantMessage::Leave => {
                    self.leave().await?;
                }
                ParticipantMessage::Close => {
                    self.close().await?;
                    return Ok(());
                }
                ParticipantMessage::ToggleAudio => {
                    self.toggle_audio().await?;
                }
                ParticipantMessage::ToggleVideo => {
                    self.toggle_video().await?;
                }
            }
        }

        Ok(())
    }

    async fn close(mut self) -> Result<()> {
        debug!(self.participant_config.name, "Closing the browser...");

        if let Err(err) = self.leave().await {
            error!(
                self.participant_config.name,
                "Failed leaving space while closing browser: {}", err
            );
        }

        self.browser.wait().await?;
        let _ = self.handle.await;

        info!(self.participant_config.name, "Closed the browser");

        self.running.store(false, Relaxed);

        Ok(())
    }

    async fn set_cookie(&self, page: &Page) -> Result<()> {
        let domain = self.participant_config.url.host_str().unwrap_or("localhost");
        let cookie = self.auth.as_browser_cookie_for(domain)?;

        page.set_cookies(vec![cookie]).await.context("failed to set cookie")?;

        debug!(self.participant_config.name, "Set cookie");

        Ok(())
    }

    async fn join(&mut self) -> Result<()> {
        // Create page
        let page = self
            .browser
            .new_page(
                CreateTargetParams::builder()
                    .url(self.participant_config.url.to_string())
                    .build()
                    .map_err(|e| eyre::eyre!(e))?,
            )
            .await
            .context("failed to create new page")?;

        debug!(self.participant_config.name, "Created new page");

        self.set_cookie(&page).await?;

        // Navigate and interact (similar to WebBrowser)
        page.goto(self.participant_config.url.to_string())
            .await
            .context("failed to wait for navigation response")?;

        debug!(self.participant_config.name, "Navigated to page");

        // Find the input box to enter the name
        let input = wait_for_element(&page, r#"[data-testid="trigger-join-name"]"#, Duration::from_secs(30))
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
            .type_str(&self.participant_config.name)
            .await
            .context("failed to insert name")?;

        debug!(self.participant_config.name, "Set the name of the participant");

        // Find the join button and click it
        wait_for_element(
            &page,
            r#"button[type="submit"]:not([disabled])"#,
            Duration::from_secs(30),
        )
        .await?
        .click()
        .await
        .context("failed to click submit button")?;

        debug!(self.participant_config.name, "Clicked on the join button");

        // Ensure we have joined the space.
        wait_for_element(&page, r#"[data-testid="trigger-leave-call"]"#, Duration::from_secs(30))
            .await
            .context("We haven't joined the space, cannot find the leave button")?;

        self.page = Some(page);

        info!(self.participant_config.name, "Joined the space");

        self.joined.store(true, Relaxed);

        Ok(())
    }

    async fn leave(&self) -> Result<()> {
        if let Some(page) = &self.page {
            page.find_element(r#"button[data-testid="trigger-leave-call"]"#)
                .await
                .context("Could not find the leave space button")?
                .click()
                .await
                .context("Could not click on the leave space button")?;

            info!(self.participant_config.name, "Left the space");

            self.joined.store(false, Relaxed);
            self.muted.store(false, Relaxed);
            self.invisible.store(false, Relaxed);
        }

        Ok(())
    }

    pub async fn toggle_audio(&self) -> Result<()> {
        if let Some(page) = &self.page {
            page.find_element(r#"button[data-testid="toggle-audio"]"#)
                .await
                .context("Could not find the toggle audio button")?
                .click()
                .await
                .context("Could not click on the toggle audio button")?;

            info!(self.participant_config.name, "Toggled audio");

            let muted = self.muted.load(Relaxed);
            self.muted.store(!muted, Relaxed);
        }

        Ok(())
    }

    pub async fn toggle_video(&self) -> Result<()> {
        if let Some(page) = &self.page {
            page.find_element(r#"div[data-testid="toggle-camera"]"#)
                .await
                .context("Could not find the toggle camera button")?
                .click()
                .await
                .context("Could not click on the toggle camera button")?;

            info!(self.participant_config.name, "Toggled camera");

            let invisible = self.invisible.load(Relaxed);
            self.invisible.store(!invisible, Relaxed);
        }

        Ok(())
    }
}
