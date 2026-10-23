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
use client_simulator_config::{
    TransportMode,
    VideoConstraint,
    VideoMaxConcurrentTracks,
};
use eyre::Result;
use futures::{
    future::BoxFuture,
    FutureExt as _,
};
use std::future::pending;

pub(super) struct RemoteStubSession {
    launch_spec: ParticipantLaunchSpec,
    state: ParticipantState,
}

impl RemoteStubSession {
    pub(super) fn new(launch_spec: ParticipantLaunchSpec) -> Self {
        Self {
            state: ParticipantState {
                username: launch_spec.username.clone(),
                ..Default::default()
            },
            launch_spec,
        }
    }

    fn log_message(&self, level: &str, message: impl ToString) {
        ParticipantLogMessage::new(level, &self.launch_spec.username, message).write();
    }
}

impl ParticipantDriverSession for RemoteStubSession {
    fn participant_name(&self) -> &str {
        &self.launch_spec.username
    }

    fn start(&mut self) -> BoxFuture<'_, Result<()>> {
        async move {
            // The stub applies none of hyper.video's settings, so a setting that the participant leaves to hyper.video
            // reports hyper.video's default, WebTransport for the transport, and no limit for the track limit.
            self.state = ParticipantState {
                username: self.launch_spec.username.clone(),
                running: true,
                warning: None,
                joined: true,
                muted: !self.launch_spec.settings.audio_enabled,
                video_activated: self.launch_spec.settings.video_enabled,
                auto_gain_control: self.launch_spec.settings.auto_gain_control.own().unwrap_or(true),
                noise_suppression: self.launch_spec.settings.noise_suppression,
                transport_mode: self
                    .launch_spec
                    .settings
                    .transport
                    .own()
                    .unwrap_or(TransportMode::WebTransport),
                video_constraint_publish_webcam: self
                    .launch_spec
                    .settings
                    .video_constraint_publish_webcam
                    .own()
                    .unwrap_or(VideoConstraint::None),
                video_constraint_subscribe: self
                    .launch_spec
                    .settings
                    .video_constraint_subscribe
                    .own()
                    .unwrap_or(VideoConstraint::None),
                video_max_concurrent_tracks: self
                    .launch_spec
                    .settings
                    .video_max_concurrent_tracks
                    .own()
                    .unwrap_or(VideoMaxConcurrentTracks::Unlimited),
                background_blur: self.launch_spec.settings.blur.own().unwrap_or(false),
                screenshare_activated: self.launch_spec.settings.screenshare_enabled,
            };

            self.log_message("warn", "remote backend is a local stub; commands are simulated locally");
            Ok(())
        }
        .boxed()
    }

    fn handle_command(&mut self, message: ParticipantMessage) -> BoxFuture<'_, Result<()>> {
        async move {
            match message {
                ParticipantMessage::Join => {
                    self.state.joined = true;
                    self.log_message("info", "remote stub join simulated");
                }
                ParticipantMessage::Leave => {
                    self.state.joined = false;
                    self.state.screenshare_activated = false;
                    self.log_message("info", "remote stub leave simulated");
                }
                ParticipantMessage::Close => {}
                ParticipantMessage::ToggleAudio => {
                    self.state.muted = !self.state.muted;
                    self.log_message("debug", "remote stub toggled audio");
                }
                ParticipantMessage::ToggleVideo => {
                    self.state.video_activated = !self.state.video_activated;
                    self.log_message("debug", "remote stub toggled video");
                }
                ParticipantMessage::ToggleScreenshare => {
                    self.state.screenshare_activated = !self.state.screenshare_activated;
                    self.log_message("debug", "remote stub toggled screenshare");
                }
                ParticipantMessage::ToggleAutoGainControl => {
                    self.state.auto_gain_control = !self.state.auto_gain_control;
                    self.log_message("debug", "remote stub toggled auto gain control");
                }
                ParticipantMessage::SetNoiseSuppression(value) => {
                    self.state.noise_suppression = value;
                    self.log_message("debug", format!("remote stub set noise suppression to {value}"));
                }
                ParticipantMessage::SetVideoConstraintPublishWebcam(value) => {
                    self.state.video_constraint_publish_webcam = value;
                    self.log_message(
                        "debug",
                        format!("remote stub set outgoing webcam video constraint to {value}"),
                    );
                }
                ParticipantMessage::SetVideoConstraintSubscribe(value) => {
                    self.state.video_constraint_subscribe = value;
                    self.log_message("debug", format!("remote stub set incoming video constraint to {value}"));
                }
                ParticipantMessage::SetVideoMaxConcurrentTracks(value) => {
                    self.state.video_max_concurrent_tracks = value;
                    self.log_message(
                        "debug",
                        format!("remote stub set max concurrent video tracks to {value}"),
                    );
                }
                ParticipantMessage::ToggleBackgroundBlur => {
                    self.state.background_blur = !self.state.background_blur;
                    self.log_message("debug", "remote stub toggled background blur");
                }
            }

            Ok(())
        }
        .boxed()
    }

    fn refresh_state(&mut self) -> BoxFuture<'_, Result<ParticipantState>> {
        async move { Ok(self.state.clone()) }.boxed()
    }

    fn close(&mut self) -> BoxFuture<'_, Result<()>> {
        async move {
            self.state.running = false;
            self.state.joined = false;
            self.state.screenshare_activated = false;
            self.log_message("debug", "remote stub closed");
            Ok(())
        }
        .boxed()
    }

    fn wait_for_termination(&mut self) -> BoxFuture<'_, DriverTermination> {
        async move { pending::<DriverTermination>().await }.boxed()
    }
}
