use crate::config::OrchestratorConfig;
use client_simulator_browser::{
    auth::{
        BorrowedCookie,
        HyperSessionCookieManger,
        HyperSessionCookieStash,
    },
    participant::transport_data::{
        FakeMediaQuery,
        ParticipantConfigQuery,
    },
};
use client_simulator_config::Config as SimClientConfig;
use color_eyre::Result;
use futures::{
    future::join_all,
    StreamExt,
};
use std::time::Duration;
use tokio::time::sleep;
use tokio_tungstenite::connect_async;
use tracing::{
    debug,
    error,
    info,
};

pub async fn run(cfg: OrchestratorConfig) -> Result<()> {
    let mut handles = Vec::new();
    let total = cfg.total_participants();
    let run_for = Duration::from_secs(cfg.run_seconds.unwrap_or(60));

    // Initialize cookie manager from the shared data dir
    let data_dir = SimClientConfig::default().data_dir().to_path_buf();
    let cookie_manager: HyperSessionCookieManger = HyperSessionCookieStash::load_from_data_dir(&data_dir).into();

    info!(total, run_secs = run_for.as_secs(), "orchestrator: starting run");

    let mut joined_participants = Vec::new();
    let start_time = tokio::time::Instant::now();

    // Main loop: check every second for participants that should join
    while joined_participants.len() < total {
        let elapsed_seconds = start_time.elapsed().as_secs();

        // Check each participant to see if it's time to join
        for idx in 0..total {
            // Skip if already joined
            if joined_participants.contains(&idx) {
                continue;
            }

            let spec = cfg.participant_spec(idx);
            let wait_time = spec.wait_to_join_seconds.unwrap_or(0);

            if elapsed_seconds >= wait_time {
                let base_url = cfg.session_url.origin().unicode_serialization();
                let effective = cfg.effective_participant(idx)?;
                let username = effective.username.clone();
                debug!(idx, user = %username, remote = %effective.remote_url, base_url, wait_time, "orchestrator: preparing participant");

                let mut query = ParticipantConfigQuery {
                    username: effective.username,
                    remote_url: effective.remote_url,
                    session_url: cfg.session_url.clone(),
                    base_url,
                    cookie: None,
                    fake_media: match effective.fake_media.as_deref() {
                        Some("none") => Some(FakeMediaQuery::None),
                        Some("builtin") => Some(FakeMediaQuery::Builtin),
                        Some(url) if url.starts_with("http") => {
                            match url::Url::parse(url) {
                                Ok(url) => Some(FakeMediaQuery::Url(url)),
                                Err(_) => Some(FakeMediaQuery::Builtin), // fallback to builtin
                            }
                        }
                        _ => Some(FakeMediaQuery::Builtin), // default to builtin
                    },
                    audio_enabled: effective.client.audio_enabled,
                    video_enabled: effective.client.video_enabled,
                    headless: effective.client.headless,
                    screenshare_enabled: effective.client.screenshare_enabled,
                    noise_suppression: effective.client.noise_suppression,
                    transport: effective.client.transport,
                    resolution: effective.client.resolution,
                    blur: effective.client.blur,
                };

                // Ensure a valid cookie in the query before connecting; keep it alive during the task
                let cookie_guard: Option<BorrowedCookie> = query.ensure_cookie(cookie_manager.clone()).await?;
                if cookie_guard.is_some() {
                    debug!(idx, "orchestrator: fetched new cookie");
                } else {
                    debug!(idx, "orchestrator: reused existing cookie");
                }

                let handle = tokio::spawn(async move {
                    let url = match query.into_url() {
                        Ok(u) => u,
                        Err(e) => {
                            error!("invalid participant URL: {e}");
                            return;
                        }
                    };
                    let (ws, _resp) = match connect_async(url.to_string()).await {
                        Ok(ok) => ok,
                        Err(e) => {
                            error!("failed to connect to worker: {e}");
                            return;
                        }
                    };
                    info!(idx, "orchestrator: connected websocket");
                    let (_sink, mut stream) = ws.split();

                    // Keep connection alive for the specified duration
                    let _recv_task = tokio::spawn(async move {
                        debug!("orchestrator: recv task started");
                        while let Some(_res) = stream.next().await { /* ignore for now */ }
                        debug!("orchestrator: recv task exiting");
                    });

                    info!(idx, "orchestrator: holding connection");
                    sleep(run_for).await;

                    // Keep cookie alive until task end
                    let _ = cookie_guard;
                    info!(idx, "orchestrator: participant finished");
                });

                handles.push(handle);
                joined_participants.push(idx);
                info!(idx, user = %username, elapsed_seconds, "orchestrator: participant joined");
            }
        }

        // Wait one second before checking again
        sleep(Duration::from_secs(1)).await;
    }

    join_all(handles).await;
    info!("orchestrator: run completed");
    Ok(())
}
