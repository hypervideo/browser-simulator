use crate::{
    auth::{
        BorrowedCookie,
        HyperSessionCookieManger,
    },
    participant::shared::{
        messages::{
            ParticipantLogMessage,
            ParticipantMessage,
        },
        DriverTermination,
        ParticipantDriverSession,
        ParticipantLaunchSpec,
        ParticipantState,
        ResolvedFrontendKind,
    },
};
use client_simulator_config::CloudflareConfig;
use cloudflare_worker_client::{
    types,
    CloudflareWorkerClient,
};
use eyre::{
    bail,
    eyre,
    Context as _,
    Result,
};
use futures::{
    future::BoxFuture,
    FutureExt as _,
};
#[cfg(test)]
use std::sync::Mutex;
use std::{
    future::pending,
    time::Duration,
};
use tokio::sync::mpsc::UnboundedSender;

enum CloudflareAuth {
    HyperCore {
        cookie: Option<BorrowedCookie>,
        cookie_manager: HyperSessionCookieManger,
    },
    HyperLite,
}

pub(super) struct CloudflareSession {
    launch_spec: ParticipantLaunchSpec,
    cloudflare_config: CloudflareConfig,
    sender: UnboundedSender<ParticipantLogMessage>,
    auth: CloudflareAuth,
    session_id: Option<String>,
    state: ParticipantState,
}

impl CloudflareSession {
    pub(super) fn new(
        launch_spec: ParticipantLaunchSpec,
        cloudflare_config: CloudflareConfig,
        sender: UnboundedSender<ParticipantLogMessage>,
        cookie: Option<BorrowedCookie>,
        cookie_manager: HyperSessionCookieManger,
    ) -> Self {
        Self::build(launch_spec, cloudflare_config, sender, cookie, cookie_manager, true)
    }

    fn build(
        launch_spec: ParticipantLaunchSpec,
        cloudflare_config: CloudflareConfig,
        sender: UnboundedSender<ParticipantLogMessage>,
        cookie: Option<BorrowedCookie>,
        cookie_manager: HyperSessionCookieManger,
        track_spawn: bool,
    ) -> Self {
        #[cfg(test)]
        {
            if track_spawn {
                spawned_participants_for_test()
                    .lock()
                    .unwrap()
                    .push(launch_spec.username.clone());
            }
        }

        let auth = match launch_spec.frontend_kind {
            ResolvedFrontendKind::HyperCore => CloudflareAuth::HyperCore { cookie, cookie_manager },
            ResolvedFrontendKind::HyperLite => CloudflareAuth::HyperLite,
        };

        Self {
            state: ParticipantState {
                username: launch_spec.username.clone(),
                ..Default::default()
            },
            launch_spec,
            cloudflare_config,
            sender,
            auth,
            session_id: None,
        }
    }

    #[cfg(test)]
    fn new_for_test(
        launch_spec: ParticipantLaunchSpec,
        cloudflare_config: CloudflareConfig,
        sender: UnboundedSender<ParticipantLogMessage>,
        cookie: Option<BorrowedCookie>,
        cookie_manager: HyperSessionCookieManger,
    ) -> Self {
        Self::build(launch_spec, cloudflare_config, sender, cookie, cookie_manager, false)
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

    fn worker_client(&self) -> Result<CloudflareWorkerClient> {
        CloudflareWorkerClient::new(
            self.cloudflare_config.base_url.as_ref(),
            Duration::from_secs(self.cloudflare_config.request_timeout_seconds),
        )
        .wrap_err("Failed to construct Cloudflare worker client")
    }

    fn log_worker_entries(&self, entries: &[types::AutomationLogEntry]) {
        for entry in entries {
            self.log_message("debug", format!("worker {} {}", entry.at.to_rfc3339(), entry.step));
        }
    }

    async fn ensure_hyper_session_cookie(&mut self) -> Result<Option<String>> {
        match &mut self.auth {
            CloudflareAuth::HyperCore { cookie, cookie_manager } => {
                if cookie.is_none() {
                    *cookie = Some(
                        cookie_manager
                            .give_or_fetch_cookie(self.launch_spec.base_url(), &self.launch_spec.username)
                            .await?,
                    );
                }

                Ok(cookie.as_ref().map(|cookie| cookie.raw_value().to_owned()))
            }
            CloudflareAuth::HyperLite => Ok(None),
        }
    }

    async fn build_create_request(&mut self) -> Result<types::SessionCreateRequest> {
        let hyper_session_cookie = self
            .ensure_hyper_session_cookie()
            .await?
            .map(types::SessionCreateRequestHyperSessionCookie::try_from)
            .transpose()
            .map_err(|error| eyre!("Failed to encode Hyper Core session cookie for the worker: {error}"))?;

        Ok(types::SessionCreateRequest {
            debug: Some(self.cloudflare_config.debug),
            display_name: types::SessionCreateRequestDisplayName::try_from(self.launch_spec.username.clone())
                .map_err(|error| eyre!("Invalid Cloudflare display name: {error}"))?,
            frontend_kind: map_frontend_kind(self.launch_spec.frontend_kind),
            hyper_session_cookie,
            navigation_timeout_ms: Some(self.cloudflare_config.navigation_timeout_ms as f64),
            room_url: self.launch_spec.session_url.to_string(),
            selector_timeout_ms: Some(self.cloudflare_config.selector_timeout_ms as f64),
            session_timeout_ms: Some(self.cloudflare_config.session_timeout_ms as f64),
            settings: map_settings(&self.launch_spec.settings),
        })
    }

    async fn start_inner(&mut self) -> Result<()> {
        if self.session_id.is_some() {
            bail!("Cloudflare session already started");
        }

        self.log_message(
            "info",
            format!(
                "Creating Cloudflare worker session via {}",
                self.cloudflare_config.base_url
            ),
        );

        let request = self.build_create_request().await?;
        let response = self.worker_client()?.create_session(&request).await?;
        self.log_worker_entries(&response.log);

        self.state = map_state(&response.state);
        self.session_id = Some(response.session_id.clone());

        self.log_message(
            "info",
            format!("Created Cloudflare worker session {}", response.session_id),
        );

        Ok(())
    }

    async fn close_inner(&mut self) -> Result<()> {
        let Some(session_id) = self.session_id.clone() else {
            self.log_message("debug", "Cloudflare worker session already closed");
            return Ok(());
        };

        self.log_message("info", format!("Closing Cloudflare worker session {session_id}"));

        let response = self.worker_client()?.close_session(&session_id).await?;
        self.log_worker_entries(&response.log);
        self.session_id = None;
        self.state.joined = false;
        self.state.screenshare_activated = false;

        self.log_message("info", format!("Closed Cloudflare worker session {session_id}"));

        Ok(())
    }
}

impl ParticipantDriverSession for CloudflareSession {
    fn participant_name(&self) -> &str {
        &self.launch_spec.username
    }

    fn start(&mut self) -> BoxFuture<'_, Result<()>> {
        self.start_inner().boxed()
    }

    fn handle_command(&mut self, message: ParticipantMessage) -> BoxFuture<'_, Result<()>> {
        async move { bail!("Cloudflare runtime command handling is not implemented yet: {message}") }.boxed()
    }

    fn refresh_state(&mut self) -> BoxFuture<'_, Result<ParticipantState>> {
        async move { Ok(self.state.clone()) }.boxed()
    }

    fn close(&mut self) -> BoxFuture<'_, Result<()>> {
        self.close_inner().boxed()
    }

    fn wait_for_termination(&mut self) -> BoxFuture<'_, DriverTermination> {
        async move { pending::<DriverTermination>().await }.boxed()
    }
}

fn map_frontend_kind(frontend_kind: ResolvedFrontendKind) -> types::SessionCreateRequestFrontendKind {
    match frontend_kind {
        ResolvedFrontendKind::HyperCore => types::SessionCreateRequestFrontendKind::HyperCore,
        ResolvedFrontendKind::HyperLite => types::SessionCreateRequestFrontendKind::HyperLite,
    }
}

fn map_settings(settings: &crate::participant::shared::ParticipantSettings) -> types::ParticipantSettings {
    types::ParticipantSettings {
        audio_enabled: settings.audio_enabled,
        blur: settings.blur,
        noise_suppression: match settings.noise_suppression {
            client_simulator_config::NoiseSuppression::Disabled => types::ParticipantSettingsNoiseSuppression::None,
            client_simulator_config::NoiseSuppression::Deepfilternet => {
                types::ParticipantSettingsNoiseSuppression::Deepfilternet
            }
            client_simulator_config::NoiseSuppression::RNNoise => types::ParticipantSettingsNoiseSuppression::Rnnoise,
            client_simulator_config::NoiseSuppression::IRISCarthy => {
                types::ParticipantSettingsNoiseSuppression::IrisCarthy
            }
            client_simulator_config::NoiseSuppression::KrispHigh => {
                types::ParticipantSettingsNoiseSuppression::KrispHigh
            }
            client_simulator_config::NoiseSuppression::KrispMedium => {
                types::ParticipantSettingsNoiseSuppression::KrispMedium
            }
            client_simulator_config::NoiseSuppression::KrispLow => types::ParticipantSettingsNoiseSuppression::KrispLow,
            client_simulator_config::NoiseSuppression::KrispHighWithBVC => {
                types::ParticipantSettingsNoiseSuppression::KrispHighWithBvc
            }
            client_simulator_config::NoiseSuppression::KrispMediumWithBVC => {
                types::ParticipantSettingsNoiseSuppression::KrispMediumWithBvc
            }
        },
        resolution: match settings.resolution {
            client_simulator_config::WebcamResolution::Auto => types::ParticipantSettingsResolution::Auto,
            client_simulator_config::WebcamResolution::P144 => types::ParticipantSettingsResolution::P144,
            client_simulator_config::WebcamResolution::P240 => types::ParticipantSettingsResolution::P240,
            client_simulator_config::WebcamResolution::P360 => types::ParticipantSettingsResolution::P360,
            client_simulator_config::WebcamResolution::P480 => types::ParticipantSettingsResolution::P480,
            client_simulator_config::WebcamResolution::P720 => types::ParticipantSettingsResolution::P720,
            client_simulator_config::WebcamResolution::P1080 => types::ParticipantSettingsResolution::P1080,
            client_simulator_config::WebcamResolution::P1440 => types::ParticipantSettingsResolution::P1440,
            client_simulator_config::WebcamResolution::P2160 => types::ParticipantSettingsResolution::P2160,
            client_simulator_config::WebcamResolution::P4320 => types::ParticipantSettingsResolution::P4320,
        },
        screenshare_enabled: settings.screenshare_enabled,
        transport: match settings.transport {
            client_simulator_config::TransportMode::WebRTC => types::ParticipantSettingsTransport::Webrtc,
            client_simulator_config::TransportMode::WebTransport => types::ParticipantSettingsTransport::Webtransport,
        },
        video_enabled: settings.video_enabled,
    }
}

fn map_state(state: &types::ParticipantState) -> ParticipantState {
    ParticipantState {
        username: String::new(),
        running: state.running,
        joined: state.joined,
        muted: state.muted,
        video_activated: state.video_activated,
        noise_suppression: match state.noise_suppression {
            types::ParticipantStateNoiseSuppression::None => client_simulator_config::NoiseSuppression::Disabled,
            types::ParticipantStateNoiseSuppression::Deepfilternet => {
                client_simulator_config::NoiseSuppression::Deepfilternet
            }
            types::ParticipantStateNoiseSuppression::Rnnoise => client_simulator_config::NoiseSuppression::RNNoise,
            types::ParticipantStateNoiseSuppression::IrisCarthy => {
                client_simulator_config::NoiseSuppression::IRISCarthy
            }
            types::ParticipantStateNoiseSuppression::KrispHigh => client_simulator_config::NoiseSuppression::KrispHigh,
            types::ParticipantStateNoiseSuppression::KrispMedium => {
                client_simulator_config::NoiseSuppression::KrispMedium
            }
            types::ParticipantStateNoiseSuppression::KrispLow => client_simulator_config::NoiseSuppression::KrispLow,
            types::ParticipantStateNoiseSuppression::KrispHighWithBvc => {
                client_simulator_config::NoiseSuppression::KrispHighWithBVC
            }
            types::ParticipantStateNoiseSuppression::KrispMediumWithBvc => {
                client_simulator_config::NoiseSuppression::KrispMediumWithBVC
            }
        },
        transport_mode: match state.transport_mode {
            types::ParticipantStateTransportMode::Webrtc => client_simulator_config::TransportMode::WebRTC,
            types::ParticipantStateTransportMode::Webtransport => client_simulator_config::TransportMode::WebTransport,
        },
        webcam_resolution: match state.webcam_resolution {
            types::ParticipantStateWebcamResolution::Auto => client_simulator_config::WebcamResolution::Auto,
            types::ParticipantStateWebcamResolution::P144 => client_simulator_config::WebcamResolution::P144,
            types::ParticipantStateWebcamResolution::P240 => client_simulator_config::WebcamResolution::P240,
            types::ParticipantStateWebcamResolution::P360 => client_simulator_config::WebcamResolution::P360,
            types::ParticipantStateWebcamResolution::P480 => client_simulator_config::WebcamResolution::P480,
            types::ParticipantStateWebcamResolution::P720 => client_simulator_config::WebcamResolution::P720,
            types::ParticipantStateWebcamResolution::P1080 => client_simulator_config::WebcamResolution::P1080,
            types::ParticipantStateWebcamResolution::P1440 => client_simulator_config::WebcamResolution::P1440,
            types::ParticipantStateWebcamResolution::P2160 => client_simulator_config::WebcamResolution::P2160,
            types::ParticipantStateWebcamResolution::P4320 => client_simulator_config::WebcamResolution::P4320,
        },
        background_blur: state.background_blur,
        screenshare_activated: state.screenshare_activated,
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

#[cfg(test)]
mod tests {
    use super::CloudflareSession;
    use crate::{
        auth::HyperSessionCookieManger,
        participant::shared::{
            ParticipantDriverSession,
            ParticipantLaunchSpec,
            ParticipantSettings,
            ResolvedFrontendKind,
        },
    };
    use chrono::Utc;
    use client_simulator_config::{
        CloudflareConfig,
        NoiseSuppression,
        TransportMode,
        WebcamResolution,
    };
    use serde_json::{
        json,
        Value,
    };
    use std::{
        collections::VecDeque,
        fs,
        path::PathBuf,
        sync::{
            Arc,
            Mutex,
        },
        time::{
            SystemTime,
            UNIX_EPOCH,
        },
    };
    use tokio::{
        io::{
            AsyncReadExt as _,
            AsyncWriteExt as _,
        },
        net::TcpListener,
        sync::mpsc::unbounded_channel,
    };
    use url::Url;

    #[derive(Clone, Debug)]
    struct CapturedRequest {
        method: String,
        path: String,
        headers: Vec<(String, String)>,
        body: String,
    }

    #[tokio::test]
    async fn start_fetches_cookie_creates_worker_session_and_close_tears_it_down() {
        let responses = VecDeque::from(vec![
            MockResponse::new(
                200,
                "Set-Cookie: hyper_session=fetched-cookie; Path=/; HttpOnly\r\n",
                "",
            ),
            MockResponse::json(200, json!({ "ok": true })),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-123",
                    "state": {
                        "running": true,
                        "joined": true,
                        "muted": false,
                        "videoActivated": true,
                        "screenshareActivated": false,
                        "noiseSuppression": "rnnoise",
                        "transportMode": "webrtc",
                        "webcamResolution": "p720",
                        "backgroundBlur": true
                    },
                    "log": [
                        {
                            "at": Utc::now().to_rfc3339(),
                            "step": "Joined the room"
                        }
                    ]
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-123",
                    "log": [
                        {
                            "at": Utc::now().to_rfc3339(),
                            "step": "Closed the browser"
                        }
                    ]
                }),
            ),
        ]);
        let (base_url, requests, server) = spawn_http_server(responses).await;
        let cookie_manager = HyperSessionCookieManger::new(unique_temp_dir().join("cookies.json"));
        let (log_sender, _log_receiver) = unbounded_channel();
        let mut session = CloudflareSession::new_for_test(
            launch_spec(ResolvedFrontendKind::HyperCore, &format!("{base_url}/room/demo")),
            CloudflareConfig {
                base_url: Url::parse(&base_url).unwrap(),
                request_timeout_seconds: 5,
                session_timeout_ms: 120_000,
                navigation_timeout_ms: 30_000,
                selector_timeout_ms: 10_000,
                debug: true,
                health_poll_interval_ms: 5_000,
            },
            log_sender,
            None,
            cookie_manager,
        );

        session.start().await.unwrap();

        let state = session.refresh_state().await.unwrap();
        assert!(state.running);
        assert!(state.joined);
        assert_eq!(state.noise_suppression, NoiseSuppression::RNNoise);
        assert_eq!(state.transport_mode, TransportMode::WebRTC);
        assert_eq!(state.webcam_resolution, WebcamResolution::P720);
        assert!(state.background_blur);

        session.close().await.unwrap();
        server.abort();

        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 4);

        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].path, "/api/v1/auth/guest?username=guest");

        assert_eq!(requests[1].method, "PUT");
        assert_eq!(requests[1].path, "/api/v1/auth/me/name");
        assert_eq!(
            header_value(&requests[1], "cookie").as_deref(),
            Some("hyper_session=fetched-cookie")
        );
        assert_eq!(
            serde_json::from_str::<Value>(&requests[1].body).unwrap(),
            json!({ "name": "cloudflare-sim" })
        );

        assert_eq!(requests[2].method, "POST");
        assert_eq!(requests[2].path, "/sessions");
        assert_eq!(
            serde_json::from_str::<Value>(&requests[2].body).unwrap(),
            json!({
                "debug": true,
                "displayName": "cloudflare-sim",
                "frontendKind": "hyper-core",
                "hyperSessionCookie": "fetched-cookie",
                "navigationTimeoutMs": 30000.0,
                "roomUrl": format!("{base_url}/room/demo"),
                "selectorTimeoutMs": 10000.0,
                "sessionTimeoutMs": 120000.0,
                "settings": {
                    "audioEnabled": true,
                    "blur": true,
                    "noiseSuppression": "rnnoise",
                    "resolution": "p720",
                    "screenshareEnabled": false,
                    "transport": "webrtc",
                    "videoEnabled": true
                }
            })
        );

        assert_eq!(requests[3].method, "POST");
        assert_eq!(requests[3].path, "/sessions/cf-session-123/close");
    }

    fn launch_spec(frontend_kind: ResolvedFrontendKind, room_url: &str) -> ParticipantLaunchSpec {
        ParticipantLaunchSpec {
            username: "cloudflare-sim".to_owned(),
            session_url: Url::parse(room_url).unwrap(),
            frontend_kind,
            settings: ParticipantSettings {
                audio_enabled: true,
                video_enabled: true,
                screenshare_enabled: false,
                noise_suppression: NoiseSuppression::RNNoise,
                transport: TransportMode::WebRTC,
                resolution: WebcamResolution::P720,
                blur: true,
            },
        }
    }

    async fn spawn_http_server(
        responses: VecDeque<MockResponse>,
    ) -> (String, Arc<Mutex<Vec<CapturedRequest>>>, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_task = Arc::clone(&requests);

        let task = tokio::spawn(async move {
            let mut responses = responses;

            while let Some(response) = responses.pop_front() {
                let (mut stream, _) = listener.accept().await.unwrap();
                let request = read_request(&mut stream).await;
                requests_for_task.lock().unwrap().push(request);
                let reply = format!(
                    "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{}\r\n{}",
                    response.status,
                    status_text(response.status),
                    response.body.len(),
                    response.headers,
                    response.body,
                );
                stream.write_all(reply.as_bytes()).await.unwrap();
            }
        });

        (base_url, requests, task)
    }

    async fn read_request(stream: &mut tokio::net::TcpStream) -> CapturedRequest {
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 4096];
        let header_end;

        loop {
            let read = stream.read(&mut chunk).await.unwrap();
            assert!(read > 0, "unexpected EOF while reading request headers");
            buffer.extend_from_slice(&chunk[..read]);

            if let Some(end) = find_header_end(&buffer) {
                header_end = end;
                break;
            }
        }

        let headers_bytes = &buffer[..header_end];
        let headers_text = String::from_utf8(headers_bytes.to_vec()).unwrap();
        let mut lines = headers_text.split("\r\n");
        let request_line = lines.next().unwrap();
        let mut request_line = request_line.split_whitespace();
        let method = request_line.next().unwrap().to_owned();
        let path = request_line.next().unwrap().to_owned();

        let mut headers = Vec::new();
        let mut content_length = 0_usize;
        for line in lines.filter(|line| !line.is_empty()) {
            let (name, value) = line.split_once(':').unwrap();
            let value = value.trim().to_owned();
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.parse().unwrap();
            }
            headers.push((name.to_ascii_lowercase(), value));
        }

        let body_start = header_end + 4;
        let mut body = buffer[body_start..].to_vec();
        while body.len() < content_length {
            let read = stream.read(&mut chunk).await.unwrap();
            assert!(read > 0, "unexpected EOF while reading request body");
            body.extend_from_slice(&chunk[..read]);
        }

        CapturedRequest {
            method,
            path,
            headers,
            body: String::from_utf8(body).unwrap(),
        }
    }

    fn find_header_end(buffer: &[u8]) -> Option<usize> {
        buffer.windows(4).position(|window| window == b"\r\n\r\n")
    }

    fn header_value(request: &CapturedRequest, name: &str) -> Option<String> {
        request
            .headers
            .iter()
            .find(|(header_name, _)| header_name == &name.to_ascii_lowercase())
            .map(|(_, value)| value.clone())
    }

    fn unique_temp_dir() -> PathBuf {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("hyper-browser-simulator-cloudflare-{nonce}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn status_text(status: u16) -> &'static str {
        match status {
            200 => "OK",
            500 => "Internal Server Error",
            _ => "OK",
        }
    }

    struct MockResponse {
        status: u16,
        headers: String,
        body: String,
    }

    impl MockResponse {
        fn new(status: u16, headers: &str, body: &str) -> Self {
            Self {
                status,
                headers: headers.to_owned(),
                body: body.to_owned(),
            }
        }

        fn json(status: u16, body: Value) -> Self {
            Self {
                status,
                headers: String::new(),
                body: serde_json::to_string(&body).unwrap(),
            }
        }
    }
}
