use super::test_grid::TestGridApi;
use aws_sdk_devicefarm::{
    primitives::DateTime,
    types::{
        TestGridSession,
        TestGridSessionStatus,
    },
};
use eyre::{
    bail,
    Context as _,
    Result,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct DeviceFarmSessionInfo {
    pub arn: String,
    pub session_id: String,
    pub status: String,
    pub created: Option<String>,
    pub ended: Option<String>,
    pub age_seconds: Option<i64>,
    pub billing_minutes: Option<f64>,
    pub selenium_properties: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceFarmCloseResult {
    Closed,
    AlreadyClosed,
}

pub fn session_id_from_arn(arn: &str) -> Option<String> {
    arn.rsplit_once('/').map(|(_, session_id)| session_id.to_string())
}

pub fn selenium_properties_json(value: Option<&str>) -> Option<serde_json::Value> {
    value.map(|value| serde_json::from_str(value).unwrap_or_else(|_| serde_json::Value::String(value.to_string())))
}

pub async fn list_project_sessions(
    api: &dyn TestGridApi,
    project_arn: &str,
    status: Option<TestGridSessionStatus>,
) -> Result<Vec<DeviceFarmSessionInfo>> {
    let now_epoch_seconds = chrono::Utc::now().timestamp();
    let sessions = api.list_test_grid_sessions(project_arn, status).await?;
    Ok(sessions
        .into_iter()
        .map(|session| map_test_grid_session(session, now_epoch_seconds))
        .collect())
}

pub async fn list_active_project_sessions(
    api: &dyn TestGridApi,
    project_arn: &str,
) -> Result<Vec<DeviceFarmSessionInfo>> {
    list_project_sessions(api, project_arn, Some(TestGridSessionStatus::Active)).await
}

pub async fn close_test_grid_session(
    api: &dyn TestGridApi,
    project_arn: &str,
    url_expires_seconds: u64,
    session_id: &str,
) -> Result<DeviceFarmCloseResult> {
    let signed_url = api
        .create_test_grid_url(project_arn, url_expires_seconds)
        .await
        .context("failed to create Device Farm Test Grid URL")?;
    let signed_url = signed_url
        .parse::<url::Url>()
        .context("Device Farm returned an invalid Selenium endpoint URL")?;
    let command_uri = format!("/session/{session_id}")
        .parse::<http::Uri>()
        .context("invalid WebDriver session deletion URI")?;
    let url = signed_test_grid_command_url(&signed_url, &command_uri)?;

    let response = reqwest::Client::new()
        .delete(url)
        .send()
        .await
        .context("failed to send WebDriver delete session request")?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    if status == reqwest::StatusCode::OK || status == reqwest::StatusCode::NO_CONTENT {
        return Ok(DeviceFarmCloseResult::Closed);
    }

    if webdriver_error_is_invalid_session_id(&body) {
        return Ok(DeviceFarmCloseResult::AlreadyClosed);
    }

    if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::METHOD_NOT_ALLOWED {
        bail!(
            "AWS Device Farm did not accept WebDriver session deletion for {session_id} (HTTP {status}); \
             Test Grid has no supported per-session force-close API response: {body}"
        );
    }

    bail!("failed to close Device Farm session {session_id} (HTTP {status}): {body}");
}

pub(crate) fn signed_test_grid_command_url(signed_url: &url::Url, command_uri: &http::Uri) -> Result<url::Url> {
    let mut url = signed_url.clone();
    let command_path = webdriver_command_path(signed_url, command_uri);
    let base_path = signed_url.path().trim_end_matches('/');
    let path = if base_path.is_empty() {
        format!("/{command_path}")
    } else if command_path.is_empty() {
        base_path.to_string()
    } else {
        format!("{base_path}/{command_path}")
    };
    url.set_path(&path);

    let query = match (signed_url.query(), command_uri.query()) {
        (Some(signed), Some(command)) if !command.is_empty() => Some(format!("{signed}&{command}")),
        (Some(signed), _) => Some(signed.to_string()),
        (None, Some(command)) if !command.is_empty() => Some(command.to_string()),
        _ => None,
    };
    url.set_query(query.as_deref());

    Ok(url)
}

fn map_test_grid_session(session: TestGridSession, now_epoch_seconds: i64) -> DeviceFarmSessionInfo {
    let arn = session.arn().unwrap_or_default().to_string();
    let created = session.created();
    let ended = session.ended();
    DeviceFarmSessionInfo {
        session_id: session_id_from_arn(&arn).unwrap_or_default(),
        arn,
        status: session
            .status()
            .map(ToString::to_string)
            .unwrap_or_else(|| "UNKNOWN".to_string()),
        created: created.map(ToString::to_string),
        ended: ended.map(ToString::to_string),
        age_seconds: age_seconds(created, ended, now_epoch_seconds),
        billing_minutes: session.billing_minutes(),
        selenium_properties: selenium_properties_json(session.selenium_properties()),
    }
}

fn age_seconds(created: Option<&DateTime>, ended: Option<&DateTime>, now_epoch_seconds: i64) -> Option<i64> {
    let created = created?;
    let end_epoch_seconds = ended.map_or(now_epoch_seconds, DateTime::secs);
    Some(end_epoch_seconds - created.secs())
}

fn webdriver_command_path(signed_url: &url::Url, command_uri: &http::Uri) -> String {
    let joined_path = command_uri.path();
    let base_path = signed_url.path().trim_end_matches('/');

    if !base_path.is_empty() {
        let base_prefix = format!("{base_path}/");
        if let Some(command) = joined_path.strip_prefix(&base_prefix) {
            return command.trim_start_matches('/').to_string();
        }

        if let Some((parent, _)) = base_path.rsplit_once('/') {
            let parent_prefix = if parent.is_empty() {
                "/".to_string()
            } else {
                format!("{parent}/")
            };
            if let Some(command) = joined_path.strip_prefix(&parent_prefix) {
                return command.trim_start_matches('/').to_string();
            }
        }
    }

    joined_path.trim_start_matches('/').to_string()
}

fn webdriver_error_is_invalid_session_id(body: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(body).is_ok_and(|value| {
        value
            .get("value")
            .and_then(|value| value.get("error"))
            .and_then(serde_json::Value::as_str)
            == Some("invalid session id")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_session_id_from_test_grid_session_arn() {
        let arn = "arn:aws:devicefarm:us-west-2:111122223333:testgrid-session:project-guid/session-guid";
        assert_eq!(session_id_from_arn(arn), Some("session-guid".to_string()));
    }

    #[test]
    fn maps_selenium_properties_to_json_when_possible() {
        let value = selenium_properties_json(Some(r#"{"browserName":"chrome"}"#));
        assert_eq!(value, Some(serde_json::json!({"browserName": "chrome"})));
    }

    #[test]
    fn keeps_non_json_selenium_properties_as_string() {
        let value = selenium_properties_json(Some("browserName=chrome"));
        assert_eq!(value, Some(serde_json::json!("browserName=chrome")));
    }
}
