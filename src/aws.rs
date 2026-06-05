use clap::{
    Args,
    Subcommand,
};
use client_simulator_browser::participant::device_farm::{
    self,
    AwsTestGrid,
    DeviceFarmCloseResult,
    DeviceFarmSessionInfo,
};
use client_simulator_config::{
    Config,
    TuiArgs,
};
use eyre::{
    bail,
    Context as _,
    Result,
};
use serde::Serialize;
use tracing_subscriber::{
    fmt,
    prelude::*,
    registry,
    EnvFilter,
};

const DEVICE_FARM_DESKTOP_INSTANCE_MINUTE_USD: f64 = 0.005;

#[derive(Args, Debug, Clone)]
pub struct AwsArgs {
    #[command(subcommand)]
    pub command: AwsCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub enum AwsCommand {
    /// List AWS Device Farm Test Grid sessions for the configured project.
    ListSessions(ListSessionsArgs),
    /// Force-close AWS Device Farm Test Grid sessions for the configured project.
    CloseSessions(CloseSessionsArgs),
}

#[derive(Args, Debug, Clone)]
pub struct ListSessionsArgs {
    /// Print machine-readable JSON instead of a human table.
    #[clap(long)]
    pub json: bool,
    /// Filter sessions by Device Farm status.
    #[clap(long, value_enum, default_value_t = ListSessionsStatus::Active)]
    pub status: ListSessionsStatus,
    /// Only show sessions created at or after this local date/time, RFC3339 timestamp, or relative duration.
    #[clap(long, value_name = "DATE_OR_DURATION", value_parser = parse_since_arg)]
    pub since: Option<SinceDate>,
}

#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListSessionsStatus {
    Active,
    Closed,
    All,
}

impl ListSessionsStatus {
    fn test_grid_status(self) -> Option<device_farm::TestGridSessionStatus> {
        match self {
            Self::Active => Some(device_farm::TestGridSessionStatus::Active),
            Self::Closed => Some(device_farm::TestGridSessionStatus::Closed),
            Self::All => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SinceDate {
    epoch_seconds: i64,
}

#[derive(Args, Debug, Clone)]
pub struct CloseSessionsArgs {
    /// Comma-separated Device Farm session IDs. If absent, all active sessions are closed.
    #[clap(value_name = "SESSION_IDS", value_delimiter = ',')]
    pub session_ids: Vec<String>,
}

pub async fn run(args: AwsArgs, filter: EnvFilter) -> Result<()> {
    init_logging(filter)?;

    let config = Config::new(TuiArgs::default()).context("Failed to create config")?;
    if config.device_farm.project_arn.trim().is_empty() {
        bail!("device_farm.project_arn is not configured; set it in config.yaml before using `aws`");
    }

    let api = AwsTestGrid::new(&config.device_farm.region);
    match args.command {
        AwsCommand::ListSessions(args) => list_sessions(&api, &config.device_farm.project_arn, args).await,
        AwsCommand::CloseSessions(args) => close_sessions(&api, &config, args).await,
    }
}

async fn list_sessions(api: &AwsTestGrid, project_arn: &str, args: ListSessionsArgs) -> Result<()> {
    let sessions = device_farm::list_project_sessions(api, project_arn, args.status.test_grid_status()).await?;
    let sessions = filter_sessions_since(sessions, args.since.as_ref());
    if args.json {
        print!("{}", format_sessions_json(&sessions)?);
    } else {
        print!("{}", format_sessions_human(&sessions));
    }
    Ok(())
}

async fn close_sessions(api: &AwsTestGrid, config: &Config, args: CloseSessionsArgs) -> Result<()> {
    let listed_sessions = if args.session_ids.is_empty() {
        device_farm::list_active_project_sessions(api, &config.device_farm.project_arn).await?
    } else {
        Vec::new()
    };
    let targets = close_targets(&args.session_ids, &listed_sessions);

    if targets.is_empty() {
        println!("No active Device Farm sessions to close.");
        return Ok(());
    }

    let mut failures = Vec::new();
    for session_id in targets {
        match device_farm::close_test_grid_session(
            api,
            &config.device_farm.project_arn,
            config.device_farm.url_expires_seconds,
            &session_id,
        )
        .await
        {
            Ok(DeviceFarmCloseResult::Closed) => println!("closed {session_id}"),
            Ok(DeviceFarmCloseResult::AlreadyClosed) => println!("already closed {session_id}"),
            Err(err) => {
                println!("failed {session_id}: {err}");
                failures.push(format!("{session_id}: {err}"));
            }
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        bail!("failed to close Device Farm sessions: {}", failures.join("; "))
    }
}

fn init_logging(filter: EnvFilter) -> Result<()> {
    registry()
        .with(
            fmt::layer()
                .with_writer(std::io::stderr)
                .with_span_events(fmt::format::FmtSpan::CLOSE)
                .with_filter(filter),
        )
        .with(tracing_error::ErrorLayer::default())
        .try_init()?;

    Ok(())
}

fn format_sessions_json(sessions: &[DeviceFarmSessionInfo]) -> Result<String> {
    let summary = SessionListOutput::new(sessions);
    let mut output = serde_json::to_string_pretty(&summary)?;
    output.push('\n');
    Ok(output)
}

fn format_sessions_human(sessions: &[DeviceFarmSessionInfo]) -> String {
    if sessions.is_empty() {
        return format!(
            "No Device Farm sessions found.\n{}\n",
            cost_summary_text(total_billing_minutes(sessions))
        );
    }

    let headers = ["ID", "STATUS", "CREATED", "AGE", "ENDED", "BILLING"];
    let rows = sessions
        .iter()
        .map(|session| {
            [
                session.session_id.clone(),
                session.status.clone(),
                session.created.clone().unwrap_or_else(|| "-".to_string()),
                age_text(session.age_seconds),
                session.ended.clone().unwrap_or_else(|| "-".to_string()),
                billing_text(session.billing_minutes),
            ]
        })
        .collect::<Vec<_>>();
    let widths = (0..headers.len())
        .map(|index| {
            rows.iter()
                .map(|row| row[index].len())
                .max()
                .unwrap_or_default()
                .max(headers[index].len())
        })
        .collect::<Vec<_>>();

    let mut output = String::new();
    write_table_row(&mut output, &headers, &widths);
    for row in &rows {
        write_table_row(&mut output, row, &widths);
    }
    output.push_str(&cost_summary_text(total_billing_minutes(sessions)));
    output.push('\n');
    output
}

fn write_table_row<const N: usize>(output: &mut String, columns: &[impl AsRef<str>; N], widths: &[usize]) {
    for (index, column) in columns.iter().enumerate() {
        if index > 0 {
            output.push_str("  ");
        }
        let value = column.as_ref();
        output.push_str(value);
        for _ in value.len()..widths[index] {
            output.push(' ');
        }
    }
    output.push('\n');
}

fn age_text(seconds: Option<i64>) -> String {
    let Some(seconds) = seconds.filter(|seconds| *seconds >= 0) else {
        return "-".to_string();
    };
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;

    if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}

fn billing_text(minutes: Option<f64>) -> String {
    minutes.map_or_else(|| "-".to_string(), |minutes| format!("{minutes:.2}"))
}

fn parse_since_arg(value: &str) -> std::result::Result<SinceDate, String> {
    parse_since_epoch_seconds(value)
        .map(|epoch_seconds| SinceDate { epoch_seconds })
        .ok_or_else(|| {
            "expected YYYY-MM-DD, YYYY-MM-DD HH:MM[:SS], YYYY-MM-DDTHH:MM[:SS], RFC3339 timestamp, or human duration like '1 day'".to_string()
        })
}

fn parse_since_epoch_seconds(value: &str) -> Option<i64> {
    use chrono::{
        Local,
        NaiveDate,
        NaiveDateTime,
        TimeZone as _,
    };

    if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(value) {
        return Some(parsed.timestamp());
    }

    for format in [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
    ] {
        if let Ok(parsed) = NaiveDateTime::parse_from_str(value, format) {
            return Local.from_local_datetime(&parsed).single().map(|date| date.timestamp());
        }
    }

    if let Some(timestamp) = NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .and_then(|date| Local.from_local_datetime(&date).single())
        .map(|date| date.timestamp())
    {
        return Some(timestamp);
    }

    let duration = humantime::parse_duration(value).ok()?;
    let duration = chrono::Duration::from_std(duration).ok()?;
    Local::now().checked_sub_signed(duration).map(|date| date.timestamp())
}

fn filter_sessions_since(
    sessions: Vec<DeviceFarmSessionInfo>,
    since: Option<&SinceDate>,
) -> Vec<DeviceFarmSessionInfo> {
    let Some(since) = since else {
        return sessions;
    };

    sessions
        .into_iter()
        .filter(|session| {
            session
                .created
                .as_deref()
                .and_then(parse_created_epoch_seconds)
                .is_some_and(|created| created >= since.epoch_seconds)
        })
        .collect()
}

fn parse_created_epoch_seconds(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.timestamp())
}

fn total_billing_minutes(sessions: &[DeviceFarmSessionInfo]) -> f64 {
    normalize_zero(sessions.iter().filter_map(|session| session.billing_minutes).sum())
}

fn total_cost_usd(total_billing_minutes: f64) -> f64 {
    normalize_zero(total_billing_minutes * DEVICE_FARM_DESKTOP_INSTANCE_MINUTE_USD)
}

fn normalize_zero(value: f64) -> f64 {
    if value.abs() < f64::EPSILON {
        0.0
    } else {
        value
    }
}

fn cost_summary_text(total_billing_minutes: f64) -> String {
    format!(
        "Total cost: ${:.4} ({:.2} billing minutes at ${:.4}/min)",
        total_cost_usd(total_billing_minutes),
        total_billing_minutes,
        DEVICE_FARM_DESKTOP_INSTANCE_MINUTE_USD
    )
}

#[derive(Serialize)]
struct SessionListOutput<'a> {
    sessions: &'a [DeviceFarmSessionInfo],
    total_billing_minutes: f64,
    total_cost_usd: f64,
    instance_minute_usd: f64,
}

impl<'a> SessionListOutput<'a> {
    fn new(sessions: &'a [DeviceFarmSessionInfo]) -> Self {
        let total_billing_minutes = total_billing_minutes(sessions);
        Self {
            sessions,
            total_billing_minutes,
            total_cost_usd: total_cost_usd(total_billing_minutes),
            instance_minute_usd: DEVICE_FARM_DESKTOP_INSTANCE_MINUTE_USD,
        }
    }
}

fn close_targets(requested_session_ids: &[String], sessions: &[DeviceFarmSessionInfo]) -> Vec<String> {
    let candidates = if requested_session_ids.is_empty() {
        sessions
            .iter()
            .filter(|session| session.status == "ACTIVE")
            .map(|session| session.session_id.clone())
            .collect::<Vec<_>>()
    } else {
        requested_session_ids.to_vec()
    };

    let mut targets = Vec::new();
    for session_id in candidates {
        if !session_id.is_empty() && !targets.contains(&session_id) {
            targets.push(session_id);
        }
    }
    targets
}

#[cfg(test)]
mod tests {
    use super::*;
    use client_simulator_browser::participant::device_farm::DeviceFarmSessionInfo;
    use serde_json::json;

    #[test]
    fn human_output_includes_session_table_columns() {
        let output = format_sessions_human(&[session_fixture("session-1", "ACTIVE")]);

        assert!(output.contains("ID"));
        assert!(output.contains("STATUS"));
        assert!(output.contains("CREATED"));
        assert!(output.contains("AGE"));
        assert!(output.contains("BILLING"));
        assert!(output.contains("session-1"));
        assert!(output.contains("ACTIVE"));
        assert!(output.contains("12m 34s"));
    }

    #[test]
    fn json_output_includes_sessions_and_total_cost() {
        let output = format_sessions_json(&[session_fixture_with_billing("session-1", "ACTIVE", 12.5)]).unwrap();
        let value: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(
            value["sessions"],
            json!([session_fixture_with_billing("session-1", "ACTIVE", 12.5)])
        );
        assert_eq!(value["total_billing_minutes"], json!(12.5));
        assert_eq!(value["total_cost_usd"], json!(0.0625));
        assert_eq!(value["instance_minute_usd"], json!(0.005));
    }

    #[test]
    fn empty_list_output_is_explicit() {
        assert_eq!(
            format_sessions_human(&[]),
            "No Device Farm sessions found.\nTotal cost: $0.0000 (0.00 billing minutes at $0.0050/min)\n"
        );
        let value: serde_json::Value = serde_json::from_str(&format_sessions_json(&[]).unwrap()).unwrap();
        assert_eq!(value["sessions"], json!([]));
        assert_eq!(value["total_cost_usd"], json!(0.0));
    }

    #[test]
    fn human_output_includes_total_cost_for_shown_sessions() {
        let output = format_sessions_human(&[
            session_fixture_with_billing("session-1", "CLOSED", 10.0),
            session_fixture_with_billing("session-2", "CLOSED", 20.5),
        ]);

        assert!(output.contains("Total cost: $0.1525 (30.50 billing minutes at $0.0050/min)"));
    }

    #[test]
    fn list_status_maps_to_aws_status_filter() {
        assert_eq!(
            ListSessionsStatus::Active.test_grid_status(),
            Some(device_farm::TestGridSessionStatus::Active)
        );
        assert_eq!(
            ListSessionsStatus::Closed.test_grid_status(),
            Some(device_farm::TestGridSessionStatus::Closed)
        );
        assert_eq!(ListSessionsStatus::All.test_grid_status(), None);
    }

    #[test]
    fn parse_since_accepts_date_and_optional_time() {
        assert!(parse_since_arg("2026-06-04").is_ok());
        assert!(parse_since_arg("2026-06-04 10:11").is_ok());
        assert!(parse_since_arg("2026-06-04T10:11:12").is_ok());
        assert!(parse_since_arg("2026-06-04T10:11:12Z").is_ok());
    }

    #[test]
    fn parse_since_accepts_human_duration_relative_to_now() {
        let before = chrono::Local::now().timestamp();
        let since = parse_since_arg("1 day").unwrap();
        let after = chrono::Local::now().timestamp();

        assert!(since.epoch_seconds >= before - 86_400);
        assert!(since.epoch_seconds <= after - 86_400);
    }

    #[test]
    fn parse_since_rejects_unknown_text() {
        assert!(parse_since_arg("last tuesday").is_err());
    }

    #[test]
    fn since_filter_keeps_sessions_created_at_or_after_threshold() {
        let since = parse_since_arg("2026-06-04T10:11:12Z").unwrap();
        let sessions = vec![
            session_fixture_with_created("before", "CLOSED", "2026-06-04T10:11:11Z"),
            session_fixture_with_created("at", "CLOSED", "2026-06-04T10:11:12Z"),
            session_fixture_with_created("after", "CLOSED", "2026-06-04T10:11:13Z"),
            DeviceFarmSessionInfo {
                created: None,
                ..session_fixture("missing-created", "CLOSED")
            },
        ];

        let filtered = filter_sessions_since(sessions, Some(&since));
        let ids = filtered
            .iter()
            .map(|session| session.session_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(ids, vec!["at", "after"]);
    }

    #[test]
    fn provided_close_ids_target_exactly_those_ids() {
        let targets = close_targets(
            &["a".to_string(), "b".to_string()],
            &[session_fixture("active-session", "ACTIVE")],
        );

        assert_eq!(targets, vec!["a", "b"]);
    }

    #[test]
    fn empty_close_ids_selects_all_active_sessions() {
        let targets = close_targets(
            &[],
            &[
                session_fixture("active-1", "ACTIVE"),
                session_fixture("closed-1", "CLOSED"),
                session_fixture("active-2", "ACTIVE"),
            ],
        );

        assert_eq!(targets, vec!["active-1", "active-2"]);
    }

    #[test]
    fn empty_close_ids_do_not_select_closed_or_errored_sessions() {
        let targets = close_targets(
            &[],
            &[
                session_fixture("closed-1", "CLOSED"),
                session_fixture("errored-1", "ERRORED"),
            ],
        );

        assert!(targets.is_empty());
    }

    #[test]
    fn close_targets_deduplicates_preserving_first_seen_order() {
        let targets = close_targets(
            &["b".to_string(), "a".to_string(), "b".to_string(), "c".to_string()],
            &[],
        );

        assert_eq!(targets, vec!["b", "a", "c"]);
    }

    fn session_fixture(session_id: &str, status: &str) -> DeviceFarmSessionInfo {
        DeviceFarmSessionInfo {
            arn: format!("arn:aws:devicefarm:us-west-2:123456789012:testgrid-session:project/{session_id}"),
            session_id: session_id.to_string(),
            status: status.to_string(),
            created: Some("2026-06-04T10:11:12Z".to_string()),
            ended: None,
            age_seconds: Some(754),
            billing_minutes: None,
            selenium_properties: Some(json!({"browserName": "chrome"})),
        }
    }

    fn session_fixture_with_billing(session_id: &str, status: &str, billing_minutes: f64) -> DeviceFarmSessionInfo {
        DeviceFarmSessionInfo {
            billing_minutes: Some(billing_minutes),
            ..session_fixture(session_id, status)
        }
    }

    fn session_fixture_with_created(session_id: &str, status: &str, created: &str) -> DeviceFarmSessionInfo {
        DeviceFarmSessionInfo {
            created: Some(created.to_string()),
            ..session_fixture(session_id, status)
        }
    }
}
