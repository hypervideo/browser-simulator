use crate::participant::shared::{
    messages::{
        ParticipantLogMessage,
        ParticipantMessage,
    },
    DriverTermination,
    ParticipantDriverSession,
    ParticipantLaunchSpec,
    ParticipantState,
};
use client_simulator_config::CloudflareConfig;
use eyre::{
    bail,
    Result,
};
use futures::{
    future::BoxFuture,
    FutureExt as _,
};
use std::future::pending;
#[cfg(test)]
use std::sync::Mutex;
use tokio::sync::mpsc::UnboundedSender;

pub(super) struct CloudflareSession {
    launch_spec: ParticipantLaunchSpec,
    cloudflare_config: CloudflareConfig,
    sender: UnboundedSender<ParticipantLogMessage>,
    state: ParticipantState,
}

impl CloudflareSession {
    pub(super) fn new(
        launch_spec: ParticipantLaunchSpec,
        cloudflare_config: CloudflareConfig,
        sender: UnboundedSender<ParticipantLogMessage>,
    ) -> Self {
        #[cfg(test)]
        {
            spawned_participants_for_test()
                .lock()
                .unwrap()
                .push(launch_spec.username.clone());
        }

        Self {
            state: ParticipantState {
                username: launch_spec.username.clone(),
                ..Default::default()
            },
            launch_spec,
            cloudflare_config,
            sender,
        }
    }

    fn log_message(&self, level: &str, message: impl ToString) {
        let log_message = ParticipantLogMessage::new(level, &self.launch_spec.username, message);
        log_message.write();
        if let Err(err) = self.sender.send(log_message) {
            trace!(
                participant = %self.launch_spec.username,
                "Failed to send cloudflare driver log message: {err}"
            );
        }
    }
}

impl ParticipantDriverSession for CloudflareSession {
    fn participant_name(&self) -> &str {
        &self.launch_spec.username
    }

    fn start(&mut self) -> BoxFuture<'_, Result<()>> {
        async move {
            self.log_message(
                "warn",
                format!(
                    "cloudflare backend selected with worker {}; driver lifecycle is not implemented yet",
                    self.cloudflare_config.base_url
                ),
            );
            bail!("Cloudflare backend wiring is in place, but the Cloudflare driver is not implemented yet")
        }
        .boxed()
    }

    fn handle_command(&mut self, _message: ParticipantMessage) -> BoxFuture<'_, Result<()>> {
        async move { Ok(()) }.boxed()
    }

    fn refresh_state(&mut self) -> BoxFuture<'_, Result<ParticipantState>> {
        async move { Ok(self.state.clone()) }.boxed()
    }

    fn close(&mut self) -> BoxFuture<'_, Result<()>> {
        async move { Ok(()) }.boxed()
    }

    fn wait_for_termination(&mut self) -> BoxFuture<'_, DriverTermination> {
        async move { pending::<DriverTermination>().await }.boxed()
    }
}

#[cfg(test)]
fn spawned_participants_for_test() -> &'static Mutex<Vec<String>> {
    static SPAWNED: Mutex<Vec<String>> = Mutex::new(Vec::new());
    &SPAWNED
}

#[cfg(test)]
pub(crate) fn take_spawned_participants_for_test() -> Vec<String> {
    std::mem::take(&mut *spawned_participants_for_test().lock().unwrap())
}
