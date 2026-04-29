use super::{
    messages::{
        ParticipantLogMessage,
        ParticipantMessage,
    },
    ParticipantState,
};
use eyre::Result;
use futures::future::BoxFuture;
use tokio::sync::{
    mpsc::{
        UnboundedReceiver,
        UnboundedSender,
    },
    watch,
};
use tokio_util::sync::CancellationToken;

/// A backend-reported termination event that the shared runtime can log and react to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::participant) struct DriverTermination {
    pub(in crate::participant) level: &'static str,
    pub(in crate::participant) message: String,
}

impl DriverTermination {
    pub(in crate::participant) fn new(level: &'static str, message: impl Into<String>) -> Self {
        Self {
            level,
            message: message.into(),
        }
    }
}

/// Backend session interface used by the shared participant runtime.
pub(in crate::participant) trait ParticipantDriverSession: Send {
    fn participant_name(&self) -> &str;
    fn start(&mut self) -> BoxFuture<'_, Result<()>>;
    fn handle_command(&mut self, message: ParticipantMessage) -> BoxFuture<'_, Result<()>>;
    fn refresh_state(&mut self) -> BoxFuture<'_, Result<ParticipantState>>;
    fn close(&mut self) -> BoxFuture<'_, Result<()>>;
    fn wait_for_termination(&mut self) -> BoxFuture<'_, DriverTermination>;
}

/// Drive one participant session by translating runtime messages into backend operations.
pub(in crate::participant) async fn run_participant_runtime<D>(
    mut receiver: UnboundedReceiver<ParticipantMessage>,
    sender: UnboundedSender<ParticipantLogMessage>,
    state: watch::Sender<ParticipantState>,
    mut driver: D,
    cancellation_token: CancellationToken,
) -> Result<()>
where
    D: ParticipantDriverSession,
{
    state.send_modify(|current| {
        current.username = driver.participant_name().to_string();
        current.running = true;
    });

    let start_result = tokio::select! {
        biased;

        _ = cancellation_token.cancelled() => {
            if let Err(err) = driver.close().await {
                log_runtime_message(
                    &sender,
                    "error",
                    driver.participant_name(),
                    format!("Failed closing participant after task cancellation: {err}"),
                );
            }
            mark_stopped(&state);
            return Ok(());
        }
        result = driver.start() => result,
    };

    if let Err(err) = start_result {
        log_runtime_message(
            &sender,
            "error",
            driver.participant_name(),
            format!("Failed joining the session when starting the browser: {err}"),
        );
        if let Err(close_err) = driver.close().await {
            log_runtime_message(
                &sender,
                "error",
                driver.participant_name(),
                format!("Failed to clean up participant after start failure: {close_err}"),
            );
        }
        mark_stopped(&state);
        return Ok(());
    }

    sync_state(&mut driver, &state, &sender).await;

    loop {
        enum RuntimeEvent {
            Command(ParticipantMessage),
            ChannelClosed,
            Terminated(DriverTermination),
            Cancelled,
        }

        let event = tokio::select! {
            biased;

            _ = cancellation_token.cancelled() => RuntimeEvent::Cancelled,
            termination = driver.wait_for_termination() => RuntimeEvent::Terminated(termination),
            message = receiver.recv() => match message {
                Some(message) => RuntimeEvent::Command(message),
                None => RuntimeEvent::ChannelClosed,
            },
        };

        match event {
            RuntimeEvent::Terminated(termination) => {
                let participant_name = driver.participant_name().to_string();
                log_runtime_message(&sender, termination.level, &participant_name, termination.message);
                if let Err(err) = driver.close().await {
                    log_runtime_message(
                        &sender,
                        "error",
                        &participant_name,
                        format!("Failed closing participant after backend termination: {err}"),
                    );
                }
                break;
            }
            RuntimeEvent::Cancelled => {
                if let Err(err) = driver.close().await {
                    log_runtime_message(
                        &sender,
                        "error",
                        driver.participant_name(),
                        format!("Failed closing participant after task cancellation: {err}"),
                    );
                }
                break;
            }
            RuntimeEvent::ChannelClosed => {
                if let Err(err) = driver.close().await {
                    log_runtime_message(
                        &sender,
                        "error",
                        driver.participant_name(),
                        format!("Failed closing participant after channel closed: {err}"),
                    );
                }
                break;
            }
            RuntimeEvent::Command(ParticipantMessage::Close) => {
                if let Err(err) = driver.close().await {
                    log_runtime_message(
                        &sender,
                        "error",
                        driver.participant_name(),
                        format!("Failed closing participant: {err}"),
                    );
                }
                break;
            }
            RuntimeEvent::Command(message) => {
                if let Err(err) = driver.handle_command(message.clone()).await {
                    log_runtime_message(
                        &sender,
                        "error",
                        driver.participant_name(),
                        format!("Running action {message} failed with error: {err}."),
                    );
                }

                sync_state(&mut driver, &state, &sender).await;
            }
        }
    }

    mark_stopped(&state);

    Ok(())
}

/// Refresh the shared participant state from the backend and publish it to watchers.
async fn sync_state<D>(
    driver: &mut D,
    state: &watch::Sender<ParticipantState>,
    sender: &UnboundedSender<ParticipantLogMessage>,
) where
    D: ParticipantDriverSession,
{
    match driver.refresh_state().await {
        Ok(mut next_state) => {
            next_state.username = driver.participant_name().to_string();
            next_state.running = true;
            state.send_modify(|current| {
                *current = next_state;
            });
        }
        Err(err) => {
            log_runtime_message(
                sender,
                "error",
                driver.participant_name(),
                format!("Failed refreshing participant state: {err}"),
            );
        }
    }
}

/// Mark the participant as no longer running after the runtime loop exits.
fn mark_stopped(state: &watch::Sender<ParticipantState>) {
    state.send_modify(|current| {
        current.running = false;
        current.joined = false;
        current.screenshare_activated = false;
    });
}

/// Log a runtime message through tracing and forward it to the participant log channel.
fn log_runtime_message(
    sender: &UnboundedSender<ParticipantLogMessage>,
    level: &str,
    participant_name: &str,
    message: impl Into<String>,
) {
    let message = message.into();
    match level {
        "trace" => trace!(participant = %participant_name, "{message}"),
        "debug" => debug!(participant = %participant_name, "{message}"),
        "info" => info!(participant = %participant_name, "{message}"),
        "warn" => warn!(participant = %participant_name, "{message}"),
        "error" => error!(participant = %participant_name, "{message}"),
        _ => warn!(participant = %participant_name, level, "{message}"),
    }

    if let Err(err) = sender.send(ParticipantLogMessage::new(level, participant_name, &message)) {
        trace!(participant = %participant_name, "Failed to send log message: {err}");
    }
}

/// Runtime tests for command dispatch, state refresh, and termination handling.
#[cfg(test)]
mod tests {
    use super::{
        run_participant_runtime,
        DriverTermination,
        ParticipantDriverSession,
    };
    use crate::participant::shared::{
        messages::ParticipantMessage,
        ParticipantState,
    };
    use eyre::Result;
    use futures::{
        future::BoxFuture,
        FutureExt as _,
    };
    use std::{
        future::pending,
        sync::{
            atomic::{
                AtomicUsize,
                Ordering,
            },
            Arc,
        },
    };
    use tokio::sync::{
        mpsc::unbounded_channel,
        watch,
    };
    use tokio_util::sync::CancellationToken;

    struct FakeDriver {
        name: String,
        joined: bool,
        muted: bool,
        close_count: usize,
        commands: Vec<ParticipantMessage>,
    }

    impl FakeDriver {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                joined: false,
                muted: false,
                close_count: 0,
                commands: Vec::new(),
            }
        }
    }

    impl ParticipantDriverSession for FakeDriver {
        fn participant_name(&self) -> &str {
            &self.name
        }

        fn start(&mut self) -> BoxFuture<'_, Result<()>> {
            async move {
                self.joined = true;
                Ok(())
            }
            .boxed()
        }

        fn handle_command(&mut self, message: ParticipantMessage) -> BoxFuture<'_, Result<()>> {
            async move {
                self.commands.push(message.clone());
                match message {
                    ParticipantMessage::Leave => {
                        self.joined = false;
                    }
                    ParticipantMessage::ToggleAudio => {
                        self.muted = !self.muted;
                    }
                    ParticipantMessage::Join => {
                        self.joined = true;
                    }
                    ParticipantMessage::Close
                    | ParticipantMessage::ToggleVideo
                    | ParticipantMessage::ToggleScreenshare
                    | ParticipantMessage::ToggleAutoGainControl
                    | ParticipantMessage::SetNoiseSuppression(_)
                    | ParticipantMessage::SetWebcamResolutions(_)
                    | ParticipantMessage::ToggleBackgroundBlur => {}
                }
                Ok(())
            }
            .boxed()
        }

        fn refresh_state(&mut self) -> BoxFuture<'_, Result<ParticipantState>> {
            async move {
                Ok(ParticipantState {
                    username: self.name.clone(),
                    running: true,
                    joined: self.joined,
                    muted: self.muted,
                    ..Default::default()
                })
            }
            .boxed()
        }

        fn close(&mut self) -> BoxFuture<'_, Result<()>> {
            async move {
                self.close_count += 1;
                self.joined = false;
                Ok(())
            }
            .boxed()
        }

        fn wait_for_termination(&mut self) -> BoxFuture<'_, DriverTermination> {
            async move { pending::<DriverTermination>().await }.boxed()
        }
    }

    #[tokio::test]
    async fn runtime_refreshes_state_after_commands() {
        let (message_tx, message_rx) = unbounded_channel();
        let (log_tx, _log_rx) = unbounded_channel();
        let (state_tx, state_rx) = watch::channel(ParticipantState::default());

        let runtime = tokio::spawn(run_participant_runtime(
            message_rx,
            log_tx,
            state_tx,
            FakeDriver::new("sim-user"),
            CancellationToken::new(),
        ));

        message_tx.send(ParticipantMessage::ToggleAudio).unwrap();
        state_rx
            .clone()
            .wait_for(|state| state.running && state.joined && state.muted)
            .await
            .unwrap();

        message_tx.send(ParticipantMessage::Leave).unwrap();
        state_rx
            .clone()
            .wait_for(|state| state.running && !state.joined && state.muted)
            .await
            .unwrap();

        message_tx.send(ParticipantMessage::Close).unwrap();
        runtime.await.unwrap().unwrap();

        assert!(!state_rx.borrow().running);
    }

    #[tokio::test]
    async fn runtime_marks_participant_stopped_when_driver_terminates() {
        struct TerminatingDriver {
            close_count: Arc<AtomicUsize>,
            name: String,
            terminated: bool,
        }

        impl ParticipantDriverSession for TerminatingDriver {
            fn participant_name(&self) -> &str {
                &self.name
            }

            fn start(&mut self) -> BoxFuture<'_, Result<()>> {
                async move { Ok(()) }.boxed()
            }

            fn handle_command(&mut self, _message: ParticipantMessage) -> BoxFuture<'_, Result<()>> {
                async move { Ok(()) }.boxed()
            }

            fn refresh_state(&mut self) -> BoxFuture<'_, Result<ParticipantState>> {
                async move {
                    Ok(ParticipantState {
                        username: self.name.clone(),
                        running: true,
                        joined: true,
                        ..Default::default()
                    })
                }
                .boxed()
            }

            fn close(&mut self) -> BoxFuture<'_, Result<()>> {
                async move {
                    self.close_count.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
                .boxed()
            }

            fn wait_for_termination(&mut self) -> BoxFuture<'_, DriverTermination> {
                async move {
                    if self.terminated {
                        pending::<DriverTermination>().await
                    } else {
                        self.terminated = true;
                        DriverTermination::new("warn", "browser unexpectedly closed")
                    }
                }
                .boxed()
            }
        }

        let (_message_tx, message_rx) = unbounded_channel();
        let (log_tx, _log_rx) = unbounded_channel();
        let (state_tx, state_rx) = watch::channel(ParticipantState::default());
        let close_count = Arc::new(AtomicUsize::new(0));

        run_participant_runtime(
            message_rx,
            log_tx,
            state_tx,
            TerminatingDriver {
                close_count: Arc::clone(&close_count),
                name: "sim-user".to_string(),
                terminated: false,
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();

        assert_eq!(close_count.load(Ordering::SeqCst), 1);
        assert!(!state_rx.borrow().running);
        assert!(!state_rx.borrow().joined);
    }

    #[tokio::test]
    async fn runtime_closes_participant_when_task_is_cancelled() {
        struct CancelAwareDriver {
            close_count: Arc<AtomicUsize>,
        }

        impl ParticipantDriverSession for CancelAwareDriver {
            fn participant_name(&self) -> &str {
                "sim-user"
            }

            fn start(&mut self) -> BoxFuture<'_, Result<()>> {
                async move { Ok(()) }.boxed()
            }

            fn handle_command(&mut self, _message: ParticipantMessage) -> BoxFuture<'_, Result<()>> {
                async move { Ok(()) }.boxed()
            }

            fn refresh_state(&mut self) -> BoxFuture<'_, Result<ParticipantState>> {
                async move {
                    Ok(ParticipantState {
                        username: "sim-user".to_string(),
                        running: true,
                        joined: true,
                        ..Default::default()
                    })
                }
                .boxed()
            }

            fn close(&mut self) -> BoxFuture<'_, Result<()>> {
                async move {
                    self.close_count.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
                .boxed()
            }

            fn wait_for_termination(&mut self) -> BoxFuture<'_, DriverTermination> {
                async move { pending::<DriverTermination>().await }.boxed()
            }
        }

        let (_message_tx, message_rx) = unbounded_channel();
        let (log_tx, _log_rx) = unbounded_channel();
        let (state_tx, state_rx) = watch::channel(ParticipantState::default());
        let cancellation_token = CancellationToken::new();
        let close_count = Arc::new(AtomicUsize::new(0));

        let runtime = tokio::spawn(run_participant_runtime(
            message_rx,
            log_tx,
            state_tx,
            CancelAwareDriver {
                close_count: Arc::clone(&close_count),
            },
            cancellation_token.clone(),
        ));

        state_rx.clone().wait_for(|state| state.running).await.unwrap();

        cancellation_token.cancel();
        runtime.await.unwrap().unwrap();

        assert_eq!(close_count.load(Ordering::SeqCst), 1);
        assert!(!state_rx.borrow().running);
        assert!(!state_rx.borrow().joined);
    }
}
