use crate::generated::{
    types::{
        self,
        CloseSessionSessionId,
        CommandSessionSessionId,
        GetSessionStateSessionId,
        KeepAliveSessionSessionId,
    },
    Client as ApiClient,
};
use eyre::{
    eyre,
    Result,
    WrapErr,
};
use progenitor_client::Error as ApiError;
use reqwest::{
    Method,
    StatusCode,
};
use serde::Serialize;
use std::time::Duration;
use url::Url;

pub const LOCAL_WORKER_URL: &str = "http://127.0.0.1:8787";
pub const DEPLOYED_WORKER_URL: &str = "https://cloudflare-browser-simulator.hyper-video.workers.dev";

#[derive(Clone)]
pub struct CloudflareWorkerClient {
    base_url: String,
    api: ApiClient,
}

impl std::fmt::Debug for CloudflareWorkerClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CloudflareWorkerClient")
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

impl CloudflareWorkerClient {
    pub fn new(base_url: &str, request_timeout: Duration) -> Result<Self> {
        let base_url = normalize_base_url(base_url)?;
        let client = reqwest::Client::builder()
            .timeout(request_timeout)
            .build()
            .wrap_err("Failed to construct Cloudflare worker HTTP client")?;

        Ok(Self {
            api: ApiClient::new_with_client(&base_url, client),
            base_url,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn api(&self) -> &ApiClient {
        &self.api
    }

    pub async fn list_sessions(&self) -> Result<types::SessionsResponse> {
        self.api
            .list_sessions()
            .await
            .map(|response| response.into_inner())
            .map_err(|error| api_error(error, "list worker sessions", &self.base_url, &Method::GET, "sessions"))
    }

    pub async fn get_limits(&self) -> Result<types::LimitsResponse> {
        self.api
            .get_limits()
            .await
            .map(|response| response.into_inner())
            .map_err(|error| api_error(error, "fetch worker limits", &self.base_url, &Method::GET, "limits"))
    }

    pub async fn create_session(&self, request: &types::SessionCreateRequest) -> Result<types::SessionCreateResponse> {
        self.api
            .create_session(request)
            .await
            .map(|response| response.into_inner())
            .map_err(|error| {
                api_error(
                    error,
                    "create worker session",
                    &self.base_url,
                    &Method::POST,
                    "sessions",
                )
            })
    }

    pub async fn command_session(
        &self,
        session_id: &str,
        request: &types::SessionCommandRequest,
    ) -> Result<types::SessionCommandResponse> {
        let path = format!("sessions/{session_id}/commands");
        let session_id = CommandSessionSessionId::try_from(session_id)
            .map_err(|error| eyre!("Invalid worker session ID `{session_id}`: {error}"))?;

        self.api
            .command_session(&session_id, request)
            .await
            .map(|response| response.into_inner())
            .map_err(|error| api_error(error, "apply a worker command", &self.base_url, &Method::POST, &path))
    }

    pub async fn get_session_state(&self, session_id: &str) -> Result<types::SessionStateResponse> {
        let path = format!("sessions/{session_id}/state");
        let session_id = GetSessionStateSessionId::try_from(session_id)
            .map_err(|error| eyre!("Invalid worker session ID `{session_id}`: {error}"))?;

        self.api
            .get_session_state(&session_id)
            .await
            .map(|response| response.into_inner())
            .map_err(|error| api_error(error, "fetch worker state", &self.base_url, &Method::GET, &path))
    }

    pub async fn keep_alive_session(&self, session_id: &str) -> Result<types::SessionKeepAliveResponse> {
        let path = format!("sessions/{session_id}/keep-alive");
        let session_id = KeepAliveSessionSessionId::try_from(session_id)
            .map_err(|error| eyre!("Invalid worker session ID `{session_id}`: {error}"))?;

        self.api
            .keep_alive_session(&session_id)
            .await
            .map(|response| response.into_inner())
            .map_err(|error| api_error(error, "send a worker keep-alive", &self.base_url, &Method::POST, &path))
    }

    pub async fn close_session(&self, session_id: &str) -> Result<types::SessionCloseResponse> {
        let path = format!("sessions/{session_id}/close");
        let session_id = CloseSessionSessionId::try_from(session_id)
            .map_err(|error| eyre!("Invalid worker session ID `{session_id}`: {error}"))?;

        self.api
            .close_session(&session_id)
            .await
            .map(|response| response.into_inner())
            .map_err(|error| api_error(error, "close worker session", &self.base_url, &Method::POST, &path))
    }
}

fn normalize_base_url(base_url: &str) -> Result<String> {
    let url = Url::parse(base_url).wrap_err_with(|| format!("Invalid Cloudflare worker base URL: {base_url}"))?;

    if url.query().is_some() || url.fragment().is_some() {
        return Err(eyre!(
            "Invalid Cloudflare worker base URL `{base_url}`: query parameters and fragments are not supported"
        ));
    }

    Ok(url.to_string().trim_end_matches('/').to_owned())
}

fn api_error<E>(error: ApiError<E>, action: &str, base_url: &str, method: &Method, path: &str) -> eyre::Report
where
    E: Serialize + std::fmt::Debug + Send + Sync + 'static,
{
    match error {
        ApiError::ErrorResponse(response) => {
            let status = response.status();
            if status == StatusCode::NOT_FOUND {
                return eyre!(not_found_message(method, base_url, path));
            }

            let body = pretty_json(response.as_ref());
            eyre!("Failed to {action} from {base_url}: HTTP {status}\n{body}")
        }
        ApiError::InvalidResponsePayload(bytes, source) => {
            let body = String::from_utf8_lossy(&bytes);
            eyre!(
                "Failed to {action} from {base_url}: response did not match the generated API schema: {source}\n{}\n{body}",
                schema_mismatch_hint(base_url)
            )
        }
        ApiError::UnexpectedResponse(response) => {
            let status = response.status();
            if status == StatusCode::NOT_FOUND {
                eyre!(not_found_message(method, base_url, path))
            } else {
                eyre!("Failed to {action} from {base_url}: unexpected HTTP {status}")
            }
        }
        other => eyre::Report::new(other).wrap_err(format!("Failed to {action} from {base_url}")),
    }
}

fn pretty_json<T>(value: &T) -> String
where
    T: Serialize,
{
    serde_json::to_string_pretty(value).unwrap_or_else(|_| "<failed to encode JSON>".to_owned())
}

fn not_found_message(method: &Method, base_url: &str, path: &str) -> String {
    let mut message = format!("The worker at {base_url} does not expose {method} /{path}.");
    if base_url == DEPLOYED_WORKER_URL {
        message.push_str("\nUpdate the deployed worker or point the simulator at a newer local worker instance.");
    }
    message
}

fn schema_mismatch_hint(base_url: &str) -> &'static str {
    if base_url == DEPLOYED_WORKER_URL {
        "The deployed worker schema likely changed since this simulator build. Rebuild hyper-browser-simulator from the current source tree or point it at a matching local worker instance."
    } else {
        "The worker response schema likely differs from this simulator build. Rebuild hyper-browser-simulator or point it at a worker instance with a matching API schema."
    }
}

#[cfg(test)]
mod tests {
    use super::CloudflareWorkerClient;
    use crate::generated::types::{
        ParticipantSettings,
        ParticipantSettingsNoiseSuppression,
        ParticipantSettingsResolution,
        ParticipantSettingsTransport,
        SessionCreateRequest,
        SessionCreateRequestDisplayName,
        SessionCreateRequestFrontendKind,
    };
    use std::time::Duration;
    use tokio::{
        io::{
            AsyncReadExt as _,
            AsyncWriteExt as _,
        },
        net::TcpListener,
    };

    #[tokio::test]
    async fn normalizes_base_urls_before_building_requests() {
        let response = concat!(
            r#"{"ok":true,"limits":{"activeSessions":[],"maxConcurrentSessions":2,"allowedBrowserAcquisitions":1,"timeUntilNextAllowedBrowserAcquisition":0},"docs":{"activeSessions":"active","maxConcurrentSessions":"max","allowedBrowserAcquisitions":"allowed","timeUntilNextAllowedBrowserAcquisition":"wait"}}"#
        );
        let (base_url, request_task) = spawn_json_server(200, response).await;

        let client = CloudflareWorkerClient::new(&format!("{base_url}///"), Duration::from_secs(5)).unwrap();

        assert_eq!(client.base_url(), base_url);
        let _ = client.get_limits().await.unwrap();

        let request = request_task.await.unwrap();
        assert!(request.starts_with("GET /limits HTTP/1.1\r\n"), "{request}");
    }

    #[tokio::test]
    async fn translates_worker_errors_into_actionable_reports() {
        let response = r#"{"ok":false,"error":"worker exploded"}"#;
        let (base_url, _request_task) = spawn_json_server(500, response).await;
        let client = CloudflareWorkerClient::new(&base_url, Duration::from_secs(5)).unwrap();

        let error = client.create_session(&create_session_request()).await.unwrap_err();
        let message = error.to_string();

        assert!(message.contains("Failed to create worker session from"), "{message}");
        assert!(message.contains("HTTP 500 Internal Server Error"), "{message}");
        assert!(message.contains("worker exploded"), "{message}");
        assert!(message.contains(&base_url), "{message}");
    }

    #[tokio::test]
    async fn translates_schema_mismatches_into_rebuild_hints() {
        let response = concat!(
            r#"{"ok":true,"sessionId":"cf-session-123","state":{"running":true,"joined":true,"muted":false,"videoActivated":true,"screenshareActivated":false,"noiseSuppression":"future-noise-model","transportMode":"webrtc","webcamResolution":"auto","backgroundBlur":false},"log":[]}"#
        );
        let (base_url, _request_task) = spawn_json_server(200, response).await;
        let client = CloudflareWorkerClient::new(&base_url, Duration::from_secs(5)).unwrap();

        let error = client.create_session(&create_session_request()).await.unwrap_err();
        let message = error.to_string();

        assert!(
            message.contains("response did not match the generated API schema"),
            "{message}"
        );
        assert!(message.contains("Rebuild hyper-browser-simulator"), "{message}");
        assert!(message.contains("future-noise-model"), "{message}");
    }

    fn create_session_request() -> SessionCreateRequest {
        SessionCreateRequest {
            debug: Some(false),
            display_name: SessionCreateRequestDisplayName::try_from("Cloudflare Simulator").unwrap(),
            frontend_kind: SessionCreateRequestFrontendKind::HyperCore,
            hyper_session_cookie: None,
            navigation_timeout_ms: Some(45_000.0),
            room_url: "https://example.com/room".to_owned(),
            selector_timeout_ms: Some(20_000.0),
            session_timeout_ms: Some(600_000.0),
            settings: ParticipantSettings {
                audio_enabled: true,
                blur: false,
                noise_suppression: ParticipantSettingsNoiseSuppression::None,
                resolution: ParticipantSettingsResolution::Auto,
                screenshare_enabled: false,
                transport: ParticipantSettingsTransport::Webrtc,
                video_enabled: true,
            },
        }
    }

    async fn spawn_json_server(status: u16, body: &'static str) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let response = http_response(status, body);

        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = [0_u8; 4096];
            let mut request = Vec::new();

            loop {
                let read = stream.read(&mut buffer).await.unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }

            stream.write_all(response.as_bytes()).await.unwrap();
            stream.shutdown().await.unwrap();

            String::from_utf8(request).unwrap()
        });

        (format!("http://{address}"), task)
    }

    fn http_response(status: u16, body: &str) -> String {
        format!(
            "HTTP/1.1 {status} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            reason_phrase(status),
            body.len()
        )
    }

    fn reason_phrase(status: u16) -> &'static str {
        match status {
            200 => "OK",
            500 => "Internal Server Error",
            _ => "Unknown",
        }
    }
}
