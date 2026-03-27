use crate::participant::{
    messages::{
        ParticipantLogMessage,
        ParticipantMessage,
    },
    ParticipantState,
};
use client_simulator_config::ParticipantConfig;
use eyre::Result;
use tokio::sync::{
    mpsc::UnboundedReceiver,
    watch,
};

pub async fn spawn_remote_stub(
    mut receiver: UnboundedReceiver<ParticipantMessage>,
    state_sender: watch::Sender<ParticipantState>,
    participant_config: ParticipantConfig,
) -> Result<()> {
    let username = participant_config.username.clone();
    let configured_remote_url = participant_config.app_config.remote_url();

    state_sender.send_modify(|state| {
        state.username = username.clone();
        state.running = true;
        state.joined = true;
        state.muted = !participant_config.app_config.audio_enabled;
        state.video_activated = participant_config.app_config.video_enabled;
        state.noise_suppression = participant_config.app_config.noise_suppression;
        state.transport_mode = participant_config.app_config.transport;
        state.webcam_resolution = participant_config.app_config.resolution;
        state.background_blur = participant_config.app_config.blur;
        state.screenshare_activated = participant_config.app_config.screenshare_enabled;
    });

    let backend_message = match configured_remote_url {
        Some(url) => format!(
            "remote backend is a local stub; no connection will be made and configured remote URL {url} is ignored"
        ),
        None => "remote backend is a local stub; commands are simulated locally".to_string(),
    };
    ParticipantLogMessage::new("warn", &username, backend_message).write();

    while let Some(message) = receiver.recv().await {
        match message {
            ParticipantMessage::Join => {
                state_sender.send_modify(|state| {
                    state.joined = true;
                });
                ParticipantLogMessage::new("info", &username, "remote stub join simulated").write();
            }
            ParticipantMessage::Leave => {
                state_sender.send_modify(|state| {
                    state.joined = false;
                    state.screenshare_activated = false;
                });
                ParticipantLogMessage::new("info", &username, "remote stub leave simulated").write();
            }
            ParticipantMessage::Close => {
                state_sender.send_modify(|state| {
                    state.running = false;
                    state.joined = false;
                    state.screenshare_activated = false;
                });
                ParticipantLogMessage::new("debug", &username, "remote stub closed").write();
                return Ok(());
            }
            ParticipantMessage::ToggleAudio => {
                state_sender.send_modify(|state| {
                    state.muted = !state.muted;
                });
                ParticipantLogMessage::new("debug", &username, "remote stub toggled audio").write();
            }
            ParticipantMessage::ToggleVideo => {
                state_sender.send_modify(|state| {
                    state.video_activated = !state.video_activated;
                });
                ParticipantLogMessage::new("debug", &username, "remote stub toggled video").write();
            }
            ParticipantMessage::ToggleScreenshare => {
                state_sender.send_modify(|state| {
                    state.screenshare_activated = !state.screenshare_activated;
                });
                ParticipantLogMessage::new("debug", &username, "remote stub toggled screenshare").write();
            }
            ParticipantMessage::SetNoiseSuppression(value) => {
                state_sender.send_modify(|state| {
                    state.noise_suppression = value;
                });
                ParticipantLogMessage::new("debug", &username, format!("remote stub set noise suppression to {value}"))
                    .write();
            }
            ParticipantMessage::SetWebcamResolutions(value) => {
                state_sender.send_modify(|state| {
                    state.webcam_resolution = value;
                });
                ParticipantLogMessage::new("debug", &username, format!("remote stub set camera resolution to {value}"))
                    .write();
            }
            ParticipantMessage::ToggleBackgroundBlur => {
                state_sender.send_modify(|state| {
                    state.background_blur = !state.background_blur;
                });
                ParticipantLogMessage::new("debug", &username, "remote stub toggled background blur").write();
            }
        }
    }

    state_sender.send_modify(|state| {
        state.running = false;
        state.joined = false;
        state.screenshare_activated = false;
    });
    ParticipantLogMessage::new("debug", &username, "remote stub channel closed").write();

    Ok(())
}
