use clap::Args;
use client_simulator_config::{
    Config,
    NoiseSuppression,
    ParticipantBackendKind,
    TransportMode,
    TuiArgs,
    VideoConstraint,
};
use eyre::{
    Context as _,
    Result,
};
use std::future::Future;
use tokio::sync::watch;
use tracing_subscriber::{
    fmt::{
        self,
        writer::MakeWriterExt as _,
    },
    prelude::*,
    EnvFilter,
};

#[derive(Args, Debug, Clone, Default)]
pub struct HeadlessArgs {
    #[clap(long, value_name = "URL")]
    pub url: Option<url::Url>,

    #[clap(long, value_name = "BACKEND")]
    pub backend: Option<ParticipantBackendKind>,

    #[clap(long = "headless", value_parser = clap::builder::BoolishValueParser::new())]
    pub headless: Option<bool>,

    #[clap(long = "audio-enabled", value_parser = clap::builder::BoolishValueParser::new())]
    pub audio_enabled: Option<bool>,

    #[clap(long = "video-enabled", value_parser = clap::builder::BoolishValueParser::new())]
    pub video_enabled: Option<bool>,

    #[clap(long = "screenshare-enabled", value_parser = clap::builder::BoolishValueParser::new())]
    pub screenshare_enabled: Option<bool>,

    #[clap(long = "auto-gain-control", value_parser = clap::builder::BoolishValueParser::new())]
    pub auto_gain_control: Option<bool>,

    #[clap(long = "noise-suppression", value_name = "MODEL")]
    pub noise_suppression: Option<NoiseSuppression>,

    #[clap(long, value_name = "MODE")]
    pub transport: Option<TransportMode>,

    #[clap(long = "video-constraint-publish-webcam", value_name = "CONSTRAINT")]
    pub video_constraint_publish_webcam: Option<VideoConstraint>,

    #[clap(long = "video-constraint-subscribe", value_name = "CONSTRAINT")]
    pub video_constraint_subscribe: Option<VideoConstraint>,

    #[clap(long = "video-max-concurrent-tracks", value_name = "TRACKS")]
    pub video_max_concurrent_tracks: Option<usize>,

    #[clap(long, value_parser = clap::builder::BoolishValueParser::new())]
    pub blur: Option<bool>,

    #[clap(long = "participant", value_name = "JSON")]
    pub participants: Vec<String>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ParticipantOverride {
    url: Option<url::Url>,
    backend: Option<ParticipantBackendKind>,
    headless: Option<bool>,
    audio_enabled: Option<bool>,
    video_enabled: Option<bool>,
    screenshare_enabled: Option<bool>,
    auto_gain_control: Option<bool>,
    noise_suppression: Option<NoiseSuppression>,
    transport: Option<TransportMode>,
    video_constraint_publish_webcam: Option<VideoConstraint>,
    video_constraint_subscribe: Option<VideoConstraint>,
    video_max_concurrent_tracks: Option<usize>,
    blur: Option<bool>,
}

pub async fn run(args: HeadlessArgs, debug: u8) -> Result<i32> {
    init_logging(debug)?;

    let mut global_config = Config::new(TuiArgs::default()).context("Failed to create config")?;
    apply_cli_overrides(&mut global_config, &args);

    let participant_configs = build_participant_configs(global_config.clone(), &args.participants)?;
    let store = client_simulator_browser::participant::ParticipantStore::new(global_config.data_dir());

    for config in &participant_configs {
        store.spawn(config).context("Failed to spawn participant")?;
    }

    Ok(wait_for_exit(store).await)
}

fn apply_cli_overrides(config: &mut Config, args: &HeadlessArgs) {
    if let Some(url) = &args.url {
        config.url = Some(url.clone());
    }
    if let Some(backend) = args.backend {
        config.backend = backend;
    }
    if let Some(headless) = args.headless {
        config.headless = headless;
    }
    if let Some(audio_enabled) = args.audio_enabled {
        config.audio_enabled = audio_enabled;
    }
    if let Some(video_enabled) = args.video_enabled {
        config.video_enabled = video_enabled;
    }
    if let Some(screenshare_enabled) = args.screenshare_enabled {
        config.screenshare_enabled = screenshare_enabled;
    }
    if let Some(auto_gain_control) = args.auto_gain_control {
        config.auto_gain_control = auto_gain_control;
    }
    if let Some(noise_suppression) = args.noise_suppression {
        config.noise_suppression = noise_suppression;
    }
    if let Some(transport) = args.transport {
        config.transport = transport;
    }
    if let Some(value) = args.video_constraint_publish_webcam {
        config.video_constraint_publish_webcam = value;
    }
    if let Some(value) = args.video_constraint_subscribe {
        config.video_constraint_subscribe = value;
    }
    if let Some(value) = args.video_max_concurrent_tracks {
        config.video_max_concurrent_tracks = Some(value);
    }
    if let Some(blur) = args.blur {
        config.blur = blur;
    }
}

fn apply_participant_override(mut config: Config, override_: ParticipantOverride) -> Config {
    if let Some(url) = override_.url {
        config.url = Some(url);
    }
    if let Some(backend) = override_.backend {
        config.backend = backend;
    }
    if let Some(headless) = override_.headless {
        config.headless = headless;
    }
    if let Some(audio_enabled) = override_.audio_enabled {
        config.audio_enabled = audio_enabled;
    }
    if let Some(video_enabled) = override_.video_enabled {
        config.video_enabled = video_enabled;
    }
    if let Some(screenshare_enabled) = override_.screenshare_enabled {
        config.screenshare_enabled = screenshare_enabled;
    }
    if let Some(auto_gain_control) = override_.auto_gain_control {
        config.auto_gain_control = auto_gain_control;
    }
    if let Some(noise_suppression) = override_.noise_suppression {
        config.noise_suppression = noise_suppression;
    }
    if let Some(transport) = override_.transport {
        config.transport = transport;
    }
    if let Some(value) = override_.video_constraint_publish_webcam {
        config.video_constraint_publish_webcam = value;
    }
    if let Some(value) = override_.video_constraint_subscribe {
        config.video_constraint_subscribe = value;
    }
    if let Some(value) = override_.video_max_concurrent_tracks {
        config.video_max_concurrent_tracks = Some(value);
    }
    if let Some(blur) = override_.blur {
        config.blur = blur;
    }
    config
}

fn build_participant_configs(global_config: Config, participants: &[String]) -> Result<Vec<Config>> {
    if participants.is_empty() {
        return Ok(vec![global_config]);
    }

    participants
        .iter()
        .map(|json| {
            let override_: ParticipantOverride =
                serde_json::from_str(json).wrap_err_with(|| format!("Invalid --participant JSON: {json}"))?;
            Ok(apply_participant_override(global_config.clone(), override_))
        })
        .collect()
}

fn init_logging(debug: u8) -> Result<()> {
    let filter = match debug {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };
    let writer = std::io::stderr
        .with_min_level(tracing::Level::WARN)
        .or_else(std::io::stdout);

    tracing_subscriber::registry()
        .with(tracing_error::ErrorLayer::default())
        .with(
            fmt::layer()
                .with_writer(writer)
                .with_filter(EnvFilter::builder().parse_lossy(filter)),
        )
        .try_init()?;

    Ok(())
}

async fn wait_for_exit(store: client_simulator_browser::participant::ParticipantStore) -> i32 {
    wait_for_exit_with(
        wait_for_all_stopped(&store),
        wait_for_ctrl_c(),
        || store.shutdown_all(),
        wait_for_ctrl_c,
    )
    .await
}

async fn wait_for_exit_with<AllStopped, FirstSignal, ShutdownFn, Shutdown, SecondSignalFn, SecondSignal>(
    all_stopped: AllStopped,
    first_signal: FirstSignal,
    shutdown: ShutdownFn,
    second_signal: SecondSignalFn,
) -> i32
where
    AllStopped: Future<Output = ()>,
    FirstSignal: Future<Output = ()>,
    ShutdownFn: FnOnce() -> Shutdown,
    Shutdown: Future<Output = ()>,
    SecondSignalFn: FnOnce() -> SecondSignal,
    SecondSignal: Future<Output = ()>,
{
    tokio::select! {
        () = all_stopped => 0,
        () = first_signal => {
            tracing::info!("Shutting down participants. Press Ctrl-C again to force exit.");
            tokio::select! {
                () = shutdown() => 0,
                () = second_signal() => 130,
            }
        }
    }
}

async fn wait_for_ctrl_c() {
    if let Err(err) = tokio::signal::ctrl_c().await {
        tracing::error!("Failed to listen for Ctrl-C: {err}");
    }
}

async fn wait_for_all_stopped(store: &client_simulator_browser::participant::ParticipantStore) {
    let states = store
        .values()
        .into_iter()
        .map(|participant| participant.state.clone())
        .collect::<Vec<_>>();

    wait_for_states_stopped(states).await;
}

async fn wait_for_states_stopped(
    states: Vec<watch::Receiver<client_simulator_browser::participant::ParticipantState>>,
) {
    for state in states {
        wait_for_state_stopped_after_start(state).await;
    }
}

async fn wait_for_state_stopped_after_start(
    mut state: watch::Receiver<client_simulator_browser::participant::ParticipantState>,
) {
    let mut saw_running = false;

    loop {
        let running = state.borrow().running;
        if running {
            saw_running = true;
        } else if saw_running {
            return;
        }

        if state.changed().await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use client_simulator_browser::participant::ParticipantState;
    use client_simulator_config::{
        Config,
        ParticipantBackendKind,
        VideoConstraint,
    };
    use std::{
        fs,
        future::{
            pending,
            ready,
        },
        path::PathBuf,
        time::{
            SystemTime,
            UNIX_EPOCH,
        },
    };
    use url::Url;

    #[derive(Parser)]
    struct TestHeadlessCli {
        #[command(flatten)]
        args: HeadlessArgs,
    }

    #[test]
    fn cli_parsing_accepts_repeated_participants() {
        let cli = TestHeadlessCli::parse_from([
            "headless",
            "--url",
            "https://latest.dev.hyper.video/F27-T5F-DXY",
            "--participant",
            r#"{"audio_enabled": false, "backend": "local"}"#,
            "--participant",
            r#"{"audio_enabled": true, "backend": "cloudflare"}"#,
        ]);
        let args = cli.args;

        assert_eq!(
            args.url.as_ref().map(Url::as_str),
            Some("https://latest.dev.hyper.video/F27-T5F-DXY")
        );
        assert_eq!(args.participants.len(), 2);
    }

    #[test]
    fn cli_bool_options_accept_explicit_false() {
        let cli = TestHeadlessCli::parse_from(["headless", "--headless", "false", "--audio-enabled", "false"]);
        let args = cli.args;

        assert_eq!(args.headless, Some(false));
        assert_eq!(args.audio_enabled, Some(false));
    }

    #[test]
    fn cli_parsing_accepts_video_constraint_options() {
        let cli = TestHeadlessCli::parse_from([
            "headless",
            "--video-constraint-publish-webcam",
            "480p",
            "--video-constraint-subscribe",
            "720p",
            "--video-max-concurrent-tracks",
            "2",
            "--participant",
            r#"{"video_constraint_publish_webcam":"360p","video_constraint_subscribe":"none","video_max_concurrent_tracks":1}"#,
        ]);
        let args = cli.args;

        assert_eq!(args.video_constraint_publish_webcam, Some(VideoConstraint::P480));
        assert_eq!(args.video_constraint_subscribe, Some(VideoConstraint::P720));
        assert_eq!(args.video_max_concurrent_tracks, Some(2));
        assert_eq!(args.participants.len(), 1);
    }

    #[test]
    fn participant_json_overrides_global_config() {
        let global_config = Config {
            url: Some(Url::parse("https://example.com/lite/global").expect("valid url")),
            backend: ParticipantBackendKind::Local,
            audio_enabled: true,
            ..Default::default()
        };

        let configs = build_participant_configs(
            global_config,
            &[
                r#"{"url":"https://example.com/lite/participant","backend":"cloudflare","audio_enabled":false}"#
                    .to_string(),
            ],
        )
        .expect("participant configs");

        assert_eq!(configs.len(), 1);
        assert_eq!(
            configs[0].url.as_ref().map(Url::as_str),
            Some("https://example.com/lite/participant")
        );
        assert_eq!(configs[0].backend, ParticipantBackendKind::Cloudflare);
        assert!(!configs[0].audio_enabled);
    }

    #[test]
    fn participant_json_overrides_video_constraints_and_treats_null_tracks_as_absent() {
        let global_config = Config {
            video_constraint_publish_webcam: VideoConstraint::P720,
            video_constraint_subscribe: VideoConstraint::P720,
            video_max_concurrent_tracks: Some(2),
            ..Default::default()
        };

        let configs = build_participant_configs(
            global_config,
            &[r#"{"video_constraint_publish_webcam":"360p","video_max_concurrent_tracks":null}"#.to_string()],
        )
        .expect("participant configs");

        // publish overridden; subscribe omitted -> inherits global;
        // max tracks null is treated as absent -> inherits global Some(2).
        assert_eq!(configs[0].video_constraint_publish_webcam, VideoConstraint::P360);
        assert_eq!(configs[0].video_constraint_subscribe, VideoConstraint::P720);
        assert_eq!(configs[0].video_max_concurrent_tracks, Some(2));
    }

    #[test]
    fn participant_json_numeric_tracks_override_global() {
        let global_config = Config {
            video_max_concurrent_tracks: None,
            ..Default::default()
        };

        let configs = build_participant_configs(global_config, &[r#"{"video_max_concurrent_tracks":3}"#.to_string()])
            .expect("participant configs");

        assert_eq!(configs[0].video_max_concurrent_tracks, Some(3));
    }

    #[test]
    fn unknown_participant_json_field_returns_error() {
        let error = build_participant_configs(Config::default(), &[r#"{"audio_enable":false}"#.to_string()])
            .expect_err("unknown field should fail");

        assert!(error.to_string().contains("--participant"));
    }

    #[test]
    fn empty_participant_list_returns_one_effective_config() {
        let global_config = Config {
            backend: ParticipantBackendKind::RemoteStub,
            ..Default::default()
        };

        let configs = build_participant_configs(global_config, &[]).expect("participant configs");

        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].backend, ParticipantBackendKind::RemoteStub);
    }

    #[tokio::test]
    async fn wait_for_all_stopped_waits_until_running_participant_stops() {
        let data_dir = unique_test_data_dir();
        fs::create_dir_all(&data_dir).expect("create temp data dir");
        let store = client_simulator_browser::participant::ParticipantStore::new(&data_dir);
        let config = Config {
            url: Some(Url::parse("https://example.com/lite/demo").expect("valid url")),
            ..Default::default()
        };

        store.spawn_remote_stub(&config).expect("spawn remote stub");

        let wait_task = tokio::spawn({
            let store = store.clone();
            async move { wait_for_all_stopped(&store).await }
        });

        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        let finished_before_shutdown = wait_task.is_finished();

        store.shutdown_all().await;
        if !wait_task.is_finished() {
            wait_task.await.expect("wait task should finish after shutdown");
        }

        assert!(
            !finished_before_shutdown,
            "wait_for_all_stopped returned before shutdown"
        );
    }

    #[tokio::test]
    async fn wait_for_states_stopped_ignores_initial_not_running_state() {
        let (state_sender, state_receiver) = tokio::sync::watch::channel(ParticipantState::default());
        let wait_task = tokio::spawn(async move {
            wait_for_states_stopped(vec![state_receiver]).await;
        });

        tokio::task::yield_now().await;
        assert!(
            !wait_task.is_finished(),
            "initial not-running state should not finish the wait"
        );

        state_sender.send_modify(|state| state.running = true);
        tokio::task::yield_now().await;
        assert!(state_sender.borrow().running);
        assert!(!wait_task.is_finished(), "running state should not finish the wait");

        state_sender.send_modify(|state| state.running = false);
        wait_task.await.expect("wait task should finish after stopped state");
    }

    #[tokio::test]
    async fn wait_for_exit_returns_zero_when_shutdown_completes_after_signal() {
        let code = wait_for_exit_with(pending(), ready(()), || ready(()), pending).await;

        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn wait_for_exit_returns_130_when_second_signal_arrives_before_shutdown() {
        let code = wait_for_exit_with(pending(), ready(()), pending, || ready(())).await;

        assert_eq!(code, 130);
    }

    fn unique_test_data_dir() -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("current time")
            .as_nanos();
        std::env::temp_dir().join(format!("hyper-client-simulator-headless-test-{timestamp}"))
    }
}
