use client_simulator_browser::{
    auth::HyperSessionCookieManger,
    participant::{
        Participant,
        ParticipantState,
    },
};
use client_simulator_config::{
    CloudflareConfig,
    Config,
    ParticipantBackendKind,
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
        Duration,
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
    sync::watch,
    time::{
        sleep,
        timeout,
    },
};

#[tokio::test]
async fn cloudflare_runtime_updates_public_participant_state_from_worker_commands() {
    let responses = VecDeque::from(vec![
        MockResponse::json(
            200,
            json!({
                "ok": true,
                "sessionId": "cf-runtime-commands",
                "state": worker_state_json(false, false, false, "p720"),
                "log": [],
            }),
        ),
        MockResponse::json(
            200,
            json!({
                "ok": true,
                "sessionId": "cf-runtime-commands",
                "state": worker_state_json(true, false, false, "p720"),
                "log": [],
            }),
        ),
        MockResponse::json(
            200,
            json!({
                "ok": true,
                "sessionId": "cf-runtime-commands",
                "state": worker_state_json(true, true, false, "p720"),
                "log": [],
            }),
        ),
        MockResponse::json(
            200,
            json!({
                "ok": true,
                "sessionId": "cf-runtime-commands",
                "state": worker_state_json(true, true, true, "p720"),
                "log": [],
            }),
        ),
        MockResponse::json(
            200,
            json!({
                "ok": true,
                "sessionId": "cf-runtime-commands",
                "state": worker_state_json(false, true, true, "p720"),
                "log": [],
            }),
        ),
        MockResponse::json(
            200,
            json!({
                "ok": true,
                "sessionId": "cf-runtime-commands",
                "log": [],
            }),
        ),
    ]);
    let (base_url, requests, server) = spawn_http_server(responses).await;
    let cookie_manager = HyperSessionCookieManger::new(unique_temp_dir().join("cookies.json"));
    let participant = Participant::spawn(
        &cloudflare_config(&format!("{base_url}/m/demo"), &base_url, 60_000),
        cookie_manager,
    )
    .expect("cloudflare participant should spawn");
    let state = participant.state.clone();

    let started = wait_for_state(&state, |current| {
        current.running && !current.joined && current.webcam_resolution == WebcamResolution::P720
    })
    .await;
    assert!(started.running);
    assert!(!started.joined);

    participant.join();
    let joined = wait_for_state(&state, |current| current.running && current.joined).await;
    assert!(joined.joined);
    assert!(!joined.muted);

    participant.toggle_audio();
    let muted = wait_for_state(&state, |current| current.running && current.joined && current.muted).await;
    assert!(muted.muted);

    participant.toggle_video();
    let video_activated = wait_for_state(&state, |current| {
        current.running && current.joined && current.video_activated
    })
    .await;
    assert!(video_activated.video_activated);

    participant.close().await;
    assert!(!state.borrow().running);

    server.abort();

    let requests = requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 6);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/sessions");
    assert_eq!(requests[1].path, "/sessions/cf-runtime-commands/commands");
    assert_eq!(request_json(&requests[1]), json!({ "type": "join" }));
    assert_eq!(request_json(&requests[2]), json!({ "type": "toggle-audio" }));
    assert_eq!(request_json(&requests[3]), json!({ "type": "toggle-video" }));
    assert_eq!(requests[4].path, "/sessions/cf-runtime-commands/commands");
    assert_eq!(request_json(&requests[4]), json!({ "type": "leave" }));
    assert_eq!(requests[5].path, "/sessions/cf-runtime-commands/close");
}

#[tokio::test]
async fn cloudflare_runtime_survives_command_failures_and_can_still_close() {
    let responses = VecDeque::from(vec![
        MockResponse::json(
            200,
            json!({
                "ok": true,
                "sessionId": "cf-runtime-errors",
                "state": worker_state_json(false, false, false, "p720"),
                "log": [],
            }),
        ),
        MockResponse::json(
            500,
            json!({
                "ok": false,
                "error": "join exploded",
            }),
        ),
        MockResponse::json(
            200,
            json!({
                "ok": true,
                "sessionId": "cf-runtime-errors",
                "log": [],
            }),
        ),
    ]);
    let (base_url, requests, server) = spawn_http_server(responses).await;
    let cookie_manager = HyperSessionCookieManger::new(unique_temp_dir().join("cookies.json"));
    let participant = Participant::spawn(
        &cloudflare_config(&format!("{base_url}/m/demo"), &base_url, 60_000),
        cookie_manager,
    )
    .expect("cloudflare participant should spawn");
    let state = participant.state.clone();

    wait_for_state(&state, |current| {
        current.running && !current.joined && current.webcam_resolution == WebcamResolution::P720
    })
    .await;

    participant.join();
    sleep(Duration::from_millis(50)).await;

    let after_failure = state.borrow().clone();
    assert!(after_failure.running);
    assert!(!after_failure.joined);

    participant.close().await;
    assert!(!state.borrow().running);

    server.abort();

    let requests = requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[1].path, "/sessions/cf-runtime-errors/commands");
    assert_eq!(request_json(&requests[1]), json!({ "type": "join" }));
    assert_eq!(requests[2].path, "/sessions/cf-runtime-errors/close");
}

#[tokio::test]
async fn cloudflare_runtime_marks_participant_stopped_when_worker_state_poll_fails() {
    let responses = VecDeque::from(vec![
        MockResponse::json(
            200,
            json!({
                "ok": true,
                "sessionId": "cf-runtime-terminated",
                "state": worker_state_json(true, false, false, "p720"),
                "log": [],
            }),
        ),
        MockResponse::json(
            500,
            json!({
                "ok": false,
                "error": "Browser session missing",
            }),
        ),
    ]);
    let (base_url, requests, server) = spawn_http_server(responses).await;
    let cookie_manager = HyperSessionCookieManger::new(unique_temp_dir().join("cookies.json"));
    let participant = Participant::spawn(
        &cloudflare_config(&format!("{base_url}/m/demo"), &base_url, 5),
        cookie_manager,
    )
    .expect("cloudflare participant should spawn");
    let state = participant.state.clone();

    wait_for_state(&state, |current| {
        current.running && current.joined && current.webcam_resolution == WebcamResolution::P720
    })
    .await;
    wait_for_state(&state, |current| !current.running).await;

    server.abort();

    let requests = requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].path, "/sessions");
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].path, "/sessions/cf-runtime-terminated/keep-alive");
}

#[tokio::test]
async fn cloudflare_runtime_fetches_hyper_core_cookie_before_creating_worker_session() {
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
                "sessionId": "cf-runtime-core",
                "state": {
                    "running": true,
                    "joined": true,
                    "muted": false,
                    "videoActivated": true,
                    "screenshareActivated": false,
                    "autoGainControl": true,
                    "noiseSuppression": "ai-coustics-sparrow-s",
                    "transportMode": "webrtc",
                    "webcamResolution": "p1080",
                    "backgroundBlur": false,
                },
                "log": [],
            }),
        ),
        MockResponse::json(
            200,
            json!({
                "ok": true,
                "sessionId": "cf-runtime-core",
                "state": worker_state_json(false, false, false, "p720"),
                "log": [],
            }),
        ),
        MockResponse::json(
            200,
            json!({
                "ok": true,
                "sessionId": "cf-runtime-core",
                "log": [],
            }),
        ),
    ]);
    let (base_url, requests, server) = spawn_http_server(responses).await;
    let cookie_manager = HyperSessionCookieManger::new(unique_temp_dir().join("cookies.json"));
    let participant = Participant::spawn(
        &cloudflare_config(&format!("{base_url}/room/demo"), &base_url, 60_000),
        cookie_manager,
    )
    .expect("cloudflare participant should spawn");
    let state = participant.state.clone();

    let started = wait_for_state(&state, |current| {
        current.running
            && current.joined
            && current.video_activated
            && current.webcam_resolution == WebcamResolution::P1080
    })
    .await;
    assert!(started.running);
    assert!(started.joined);
    assert_eq!(
        started.noise_suppression,
        client_simulator_config::NoiseSuppression::AiCousticsSparrowS
    );

    participant.close().await;
    assert!(!state.borrow().running);

    server.abort();

    let requests = requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 5);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/api/v1/auth/guest?username=guest");
    assert_eq!(requests[1].method, "PUT");
    assert_eq!(requests[1].path, "/api/v1/auth/me/name");
    assert_eq!(
        header_value(&requests[1], "cookie").as_deref(),
        Some("hyper_session=fetched-cookie")
    );

    let set_name_body = request_json(&requests[1]);
    let display_name = request_json(&requests[2])["displayName"].clone();
    assert_eq!(requests[2].path, "/sessions");
    assert_eq!(
        request_json(&requests[2])["hyperSessionCookie"],
        json!("fetched-cookie")
    );
    assert_eq!(display_name, set_name_body["name"]);
    assert_eq!(requests[3].path, "/sessions/cf-runtime-core/commands");
    assert_eq!(request_json(&requests[3]), json!({ "type": "leave" }));
    assert_eq!(requests[4].path, "/sessions/cf-runtime-core/close");
}

fn cloudflare_config(session_url: &str, base_url: &str, health_poll_interval_ms: u64) -> Config {
    let mut config = Config::default();
    config.url = Some(session_url.parse().unwrap());
    config.backend = ParticipantBackendKind::Cloudflare;
    config.headless = true;
    config.cloudflare = CloudflareConfig {
        base_url: base_url.parse().unwrap(),
        request_timeout_seconds: 5,
        session_timeout_ms: 120_000,
        navigation_timeout_ms: 30_000,
        selector_timeout_ms: 10_000,
        debug: false,
        health_poll_interval_ms,
    };
    config
}

fn worker_state_json(joined: bool, muted: bool, video_activated: bool, webcam_resolution: &str) -> Value {
    json!({
        "running": true,
        "joined": joined,
        "muted": muted,
        "videoActivated": video_activated,
        "screenshareActivated": false,
        "autoGainControl": true,
        "noiseSuppression": "none",
        "transportMode": "webrtc",
        "webcamResolution": webcam_resolution,
        "backgroundBlur": false,
    })
}

async fn wait_for_state<F>(state: &watch::Receiver<ParticipantState>, mut predicate: F) -> ParticipantState
where
    F: FnMut(&ParticipantState) -> bool,
{
    let mut state = state.clone();
    timeout(Duration::from_secs(1), async move {
        state.wait_for(|current| predicate(current)).await.unwrap().clone()
    })
    .await
    .expect("timed out waiting for participant state")
}

#[derive(Clone, Debug)]
struct CapturedRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: String,
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

fn request_json(request: &CapturedRequest) -> Value {
    serde_json::from_str(&request.body).unwrap()
}

fn unique_temp_dir() -> PathBuf {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("hyper-browser-simulator-cloudflare-it-{nonce}"));
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
