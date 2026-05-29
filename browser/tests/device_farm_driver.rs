//! Integration test for the AWS Device Farm backend.
//!
//! Uses an in-process TCP server that speaks just enough of the W3C WebDriver
//! HTTP protocol for thirtyfour to drive a session, plus a stubbed TestGridApi
//! that points thirtyfour at that server. No real AWS or browser is involved.

use client_simulator_browser::{
    auth::HyperSessionCookieManger,
    participant::{
        Participant,
        ParticipantState,
    },
    testing::TestGridApi,
};
use client_simulator_config::{
    Config,
    DeviceFarmConfig,
    ParticipantBackendKind,
};
use eyre::Result;
use futures::{
    future::BoxFuture,
    FutureExt as _,
};
use serde_json::{
    json,
    Value,
};
use std::{
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
    time::timeout,
};

#[tokio::test]
async fn device_farm_session_creates_url_connects_joins_and_closes() {
    let (base_url, requests, server) = spawn_webdriver_mock().await;
    let cookie_manager = HyperSessionCookieManger::new(unique_temp_dir().join("cookies.json"));
    let participant = Participant::spawn_device_farm_with_api(
        &device_farm_config(),
        cookie_manager,
        Arc::new(TestGridStub { url: base_url }),
    )
    .expect("device farm participant should spawn");
    let state = participant.state.clone();

    let joined = wait_for_state(&state, |current| current.running && current.joined).await;
    assert!(joined.running);
    assert!(joined.joined);

    participant.close().await;
    assert!(!state.borrow().running);

    server.abort();

    let requests = requests.lock().unwrap().clone();
    let paths = requests
        .iter()
        .map(|request| (request.method.as_str(), request.path.as_str()))
        .collect::<Vec<_>>();
    assert!(paths.contains(&("POST", "/session")));
    assert!(paths.contains(&("POST", "/session/df-1/url")));
    assert!(paths.iter().any(|(_, path)| path == &"/session/df-1/element"));
    assert!(paths.iter().any(|(_, path)| path.ends_with("/click")));
    assert!(paths.contains(&("DELETE", "/session/df-1")));
}

#[derive(Debug)]
struct TestGridStub {
    url: String,
}

impl TestGridApi for TestGridStub {
    fn create_test_grid_url(&self, _project_arn: &str, _expires_seconds: u64) -> BoxFuture<'_, Result<String>> {
        async move { Ok(self.url.clone()) }.boxed()
    }
}

fn device_farm_config() -> Config {
    let mut config = Config::default();
    config.url = Some("https://example.com/m/demo".parse().unwrap());
    config.backend = ParticipantBackendKind::AwsDeviceFarm;
    config.headless = true;
    config.device_farm = DeviceFarmConfig {
        project_arn: "arn:aws:devicefarm:us-west-2:123456789012:testgrid-project:abc".to_string(),
        region: "us-west-2".to_string(),
        url_expires_seconds: 300,
        session_max_duration_ms: 60_000,
        idle_timeout_ms: 30_000,
        navigation_timeout_ms: 45_000,
        selector_timeout_ms: 20_000,
        health_poll_interval_ms: 30_000,
        debug: false,
    };
    config
}

async fn wait_for_state<F>(state: &watch::Receiver<ParticipantState>, mut predicate: F) -> ParticipantState
where
    F: FnMut(&ParticipantState) -> bool,
{
    let mut state = state.clone();
    timeout(Duration::from_secs(2), async move {
        state.wait_for(|current| predicate(current)).await.unwrap().clone()
    })
    .await
    .expect("timed out waiting for participant state")
}

#[derive(Clone, Debug)]
struct CapturedRequest {
    method: String,
    path: String,
    body: String,
}

#[derive(Debug, Default)]
struct WebDriverState {
    joined: bool,
}

async fn spawn_webdriver_mock() -> (String, Arc<Mutex<Vec<CapturedRequest>>>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let requests_for_task = Arc::clone(&requests);
    let state = Arc::new(Mutex::new(WebDriverState::default()));

    let task = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_request(&mut stream).await;
            let response = webdriver_response(&request, &state);
            requests_for_task.lock().unwrap().push(request);
            write_response(&mut stream, response).await;
        }
    });

    (base_url, requests, task)
}

fn webdriver_response(request: &CapturedRequest, state: &Arc<Mutex<WebDriverState>>) -> MockResponse {
    match (request.method.as_str(), request.path.as_str()) {
        ("POST", "/session") => MockResponse::json(
            200,
            json!({
                "value": {
                    "sessionId": "df-1",
                    "capabilities": {
                        "browserName": "chrome"
                    }
                }
            }),
        ),
        ("POST", "/session/df-1/url") => MockResponse::json(200, json!({ "value": null })),
        ("GET", "/session/df-1/url") => MockResponse::json(200, json!({ "value": "https://example.com/m/demo" })),
        ("DELETE", "/session/df-1") => MockResponse::json(200, json!({ "value": null })),
        ("POST", "/session/df-1/element") => element_response(request, state),
        _ if request.path.starts_with("/session/df-1/element/") => element_command_response(request, state),
        _ if request.path == "/session/df-1/execute/sync" => MockResponse::json(200, json!({ "value": null })),
        _ => MockResponse::json(200, json!({ "value": null })),
    }
}

fn element_response(request: &CapturedRequest, state: &Arc<Mutex<WebDriverState>>) -> MockResponse {
    let selector = request_json(request)["value"].as_str().unwrap_or_default().to_string();
    let joined = state.lock().unwrap().joined;

    if selector.contains("trigger-leave-call") || selector.contains("button[aria-label=\"Leave\"]") {
        if joined {
            return element("leave");
        }
        return no_such_element();
    }

    if selector.contains("join-button") {
        return element("join");
    }
    if selector.contains("meeting-lobby-display-name") {
        return element("name");
    }
    if selector.contains("toggle-audio") {
        return element("audio");
    }
    if selector.contains("toggle-video") {
        return element("video");
    }
    if selector.contains("toggle-screen-share") {
        return element("screen");
    }
    if selector.contains("alert-dialog-footer") {
        return element("confirm");
    }

    element("generic")
}

fn element_command_response(request: &CapturedRequest, state: &Arc<Mutex<WebDriverState>>) -> MockResponse {
    if request.path.ends_with("/click") {
        if request.path.contains("/element/join/") {
            state.lock().unwrap().joined = true;
        } else if request.path.contains("/element/leave/") || request.path.contains("/element/confirm/") {
            state.lock().unwrap().joined = false;
        }
        return MockResponse::json(200, json!({ "value": null }));
    }

    if let Some(attribute) = request
        .path
        .rsplit('/')
        .next()
        .filter(|_| request.path.contains("/attribute/"))
    {
        let value = match attribute {
            "data-test-state" if request.path.contains("/element/audio/") => json!("true"),
            "data-test-state" if request.path.contains("/element/video/") => json!("true"),
            "data-test-state" if request.path.contains("/element/screen/") => json!("false"),
            "aria-pressed" => Value::Null,
            "aria-label" => Value::Null,
            _ => Value::Null,
        };
        return MockResponse::json(200, json!({ "value": value }));
    }

    MockResponse::json(200, json!({ "value": null }))
}

fn element(id: &str) -> MockResponse {
    MockResponse::json(
        200,
        json!({
            "value": {
                "element-6066-11e4-a52e-4f735466cecf": id,
                "ELEMENT": id
            }
        }),
    )
}

fn no_such_element() -> MockResponse {
    MockResponse::json(
        404,
        json!({
            "value": {
                "error": "no such element",
                "message": "no such element",
                "stacktrace": ""
            }
        }),
    )
}

#[derive(Debug)]
struct MockResponse {
    status: u16,
    body: String,
}

impl MockResponse {
    fn json(status: u16, body: Value) -> Self {
        Self {
            status,
            body: serde_json::to_string(&body).unwrap(),
        }
    }
}

async fn write_response(stream: &mut tokio::net::TcpStream, response: MockResponse) {
    let reply = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status,
        status_text(response.status),
        response.body.len(),
        response.body,
    );
    stream.write_all(reply.as_bytes()).await.unwrap();
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

    let mut content_length = 0_usize;
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').unwrap();
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.trim().parse().unwrap();
        }
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
        body: String::from_utf8(body).unwrap(),
    }
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn request_json(request: &CapturedRequest) -> Value {
    serde_json::from_str(&request.body).unwrap_or(Value::Null)
}

fn unique_temp_dir() -> PathBuf {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("hyper-browser-simulator-device-farm-it-{nonce}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "OK",
    }
}
