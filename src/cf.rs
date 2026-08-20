//! `cf` subcommands: inspect and close sessions on the Cloudflare browser simulator worker.
//!
//! The worker URL and HTTP timeout come from `cloudflare.*` in config.yaml; `--base-url`, `--local`,
//! and `--timeout` override them per call. The text output matches the worker repository's own CLI.

use chrono::{
    DateTime,
    Local,
    Utc,
};
use clap::{
    Args,
    Subcommand,
};
use client_simulator_config::{
    CloudflareConfig,
    Config,
    TuiArgs,
};
use cloudflare_worker_client::{
    types::{
        Limits,
        LimitsDocs,
        Session,
        SessionSummary,
    },
    CloudflareWorkerClient,
    LOCAL_WORKER_URL,
};
use eyre::{
    bail,
    Context as _,
    Result,
};
use serde::Serialize;
use std::time::Duration;
use tracing_subscriber::{
    fmt,
    prelude::*,
    registry,
    EnvFilter,
};

const ALL_SESSIONS: &str = "all";

#[derive(Args, Debug, Clone)]
pub struct CfArgs {
    #[command(subcommand)]
    pub command: CfCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub enum CfCommand {
    /// List open worker sessions and the session summary.
    Sessions(WorkerArgs),
    /// Show the worker's Browser Rendering limits.
    Limits(WorkerArgs),
    /// Close worker sessions.
    Close(CloseArgs),
}

/// Flags shared by all `cf` commands.
#[derive(Args, Debug, Clone, Default)]
pub struct WorkerArgs {
    /// Worker base URL. Defaults to `cloudflare.base_url` from config.yaml.
    #[clap(long, value_name = "URL", conflicts_with = "local")]
    pub base_url: Option<String>,
    /// Target the local worker at http://127.0.0.1:8787.
    #[clap(long)]
    pub local: bool,
    /// HTTP timeout in seconds. Defaults to `cloudflare.request_timeout_seconds` from config.yaml.
    #[clap(long, value_name = "SECONDS")]
    pub timeout: Option<u64>,
    /// Print the raw JSON response.
    #[clap(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
pub struct CloseArgs {
    /// Session ID to close, a comma-separated list of session IDs, or `all` to close every open session.
    #[clap(value_name = "SESSION_IDS")]
    pub session_ids: String,
    #[command(flatten)]
    pub worker: WorkerArgs,
}

pub async fn run(args: CfArgs, filter: EnvFilter) -> Result<()> {
    init_logging(filter)?;
    let config = Config::new(TuiArgs::default()).context("Failed to create config")?;

    match args.command {
        CfCommand::Sessions(worker) => {
            let client = worker_client(&config.cloudflare, &worker)?;
            let response = client.list_sessions().await?;
            if worker.json {
                print_json(&response)?;
            } else {
                print!(
                    "{}",
                    format_sessions(client.base_url(), &response.summary, &response.sessions, Utc::now())
                );
            }
        }
        CfCommand::Limits(worker) => {
            let client = worker_client(&config.cloudflare, &worker)?;
            let response = client.get_limits().await?;
            if worker.json {
                print_json(&response)?;
            } else {
                print!("{}", format_limits(client.base_url(), &response.limits, &response.docs));
            }
        }
        CfCommand::Close(args) => {
            let client = worker_client(&config.cloudflare, &args.worker)?;
            let report = close_sessions(&client, &args.session_ids).await?;
            if args.worker.json {
                print_json(&report)?;
            } else {
                print!("{}", format_close_report(&report));
            }
            if !report.failures.is_empty() {
                bail!("{}", summarize_failures(&report));
            }
        }
    }

    Ok(())
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

fn worker_client(config: &CloudflareConfig, args: &WorkerArgs) -> Result<CloudflareWorkerClient> {
    let (base_url, timeout) = worker_target(config, args);
    CloudflareWorkerClient::new(&base_url, timeout).context("Failed to construct Cloudflare worker client")
}

fn worker_target(config: &CloudflareConfig, args: &WorkerArgs) -> (String, Duration) {
    let base_url = match &args.base_url {
        Some(base_url) => base_url.clone(),
        None if args.local => LOCAL_WORKER_URL.to_owned(),
        None => config.base_url.to_string(),
    };
    let timeout_seconds = args.timeout.unwrap_or(config.request_timeout_seconds);
    (base_url, Duration::from_secs(timeout_seconds))
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string(value)?);
    Ok(())
}

fn format_sessions(worker: &str, summary: &SessionSummary, sessions: &[Session], now: DateTime<Utc>) -> String {
    let mut lines = vec![
        format!("Worker: {worker}"),
        "Summary".to_owned(),
        format!("  Open sessions:            {}", number(summary.open_sessions)),
        format!("  Connected sessions:       {}", number(summary.connected_sessions)),
        format!("  Idle sessions:            {}", number(summary.idle_sessions)),
        format!(
            "  Active session slots:     {} / {}",
            number(summary.active_sessions),
            number(summary.max_concurrent_sessions)
        ),
        format!(
            "  New acquisitions allowed: {} (retry in {} ms)",
            number(summary.allowed_browser_acquisitions),
            number(summary.time_until_next_allowed_browser_acquisition)
        ),
    ];

    if sessions.is_empty() {
        lines.push("\nNo open sessions.".to_owned());
    } else {
        lines.push("\nSessions".to_owned());
    }
    for session in sessions {
        let state = if session.connection_id.is_some() {
            "connected"
        } else {
            "idle"
        };
        lines.push(format!("- {}", session.session_id));
        lines.push(format!("  Started:      {}", format_timestamp(session.start_time)));
        lines.push(format!("  Age:          {}", format_age(session.start_time, now)));
        lines.push(format!("  State:        {state}"));
        lines.push(format!(
            "  Active slot:  {}",
            if session.is_active { "yes" } else { "no" }
        ));
        if let Some(connection_id) = &session.connection_id {
            lines.push(format!("  Connection:   {connection_id}"));
            if let Some(connected_at) = session.connection_start_time {
                lines.push(format!("  Connected at: {}", format_timestamp(connected_at)));
                lines.push(format!("  Connected for: {}", format_age(connected_at, now)));
            }
        }
    }

    lines.join("\n") + "\n"
}

fn format_limits(worker: &str, limits: &Limits, docs: &LimitsDocs) -> String {
    let mut lines = vec![
        format!("Worker: {worker}"),
        "Limits".to_owned(),
        "  activeSessions".to_owned(),
        format!("    {}", docs.active_sessions),
    ];
    if limits.active_sessions.is_empty() {
        lines.push("    - none".to_owned());
    }
    lines.extend(
        limits
            .active_sessions
            .iter()
            .map(|session| format!("    - {}", session.id)),
    );
    lines.extend([
        "  maxConcurrentSessions".to_owned(),
        format!("    {}", docs.max_concurrent_sessions),
        format!("    - {}", number(limits.max_concurrent_sessions)),
        "  allowedBrowserAcquisitions".to_owned(),
        format!("    {}", docs.allowed_browser_acquisitions),
        format!("    - {}", number(limits.allowed_browser_acquisitions)),
        "  timeUntilNextAllowedBrowserAcquisition".to_owned(),
        format!("    {}", docs.time_until_next_allowed_browser_acquisition),
        format!(
            "    - {} ms",
            number(limits.time_until_next_allowed_browser_acquisition)
        ),
    ]);

    lines.join("\n") + "\n"
}

/// The worker reports counts and durations as JSON numbers; show whole values without a fraction.
fn number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

fn format_timestamp(timestamp_ms: f64) -> String {
    match DateTime::<Utc>::from_timestamp_millis(timestamp_ms as i64) {
        Some(time) => time.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S %Z").to_string(),
        None => format!("invalid timestamp {timestamp_ms}"),
    }
}

/// Elapsed time since `since_ms` as `1h 2m 3s`, leaving out zero parts.
fn format_age(since_ms: f64, now: DateTime<Utc>) -> String {
    let Some(since) = DateTime::<Utc>::from_timestamp_millis(since_ms as i64) else {
        return format!("invalid timestamp {since_ms}");
    };
    let total_seconds = (now - since).num_seconds().max(0);
    let hours = total_seconds / 3_600;
    let minutes = total_seconds % 3_600 / 60;
    let seconds = total_seconds % 60;

    let mut parts = Vec::new();
    if hours > 0 {
        parts.push(format!("{hours}h"));
    }
    if minutes > 0 {
        parts.push(format!("{minutes}m"));
    }
    if seconds > 0 || parts.is_empty() {
        parts.push(format!("{seconds}s"));
    }
    parts.join(" ")
}

#[derive(Debug, Serialize)]
struct CloseReport {
    worker: String,
    requested: String,
    matched_sessions: usize,
    closed: Vec<String>,
    failures: Vec<CloseFailure>,
}

#[derive(Debug, Serialize)]
struct CloseFailure {
    session_id: String,
    error: String,
}

async fn close_sessions(client: &CloudflareWorkerClient, requested: &str) -> Result<CloseReport> {
    let session_ids = match parse_close_target(requested)? {
        CloseTarget::All => client
            .list_sessions()
            .await?
            .sessions
            .into_iter()
            .map(|session| session.session_id)
            .collect(),
        CloseTarget::Sessions(session_ids) => session_ids,
    };

    let mut report = CloseReport {
        worker: client.base_url().to_owned(),
        requested: requested.to_owned(),
        matched_sessions: session_ids.len(),
        closed: Vec::new(),
        failures: Vec::new(),
    };
    if session_ids.is_empty() {
        return Ok(report);
    }

    for result in client.close_sessions(&session_ids).await?.results {
        if result.ok {
            report.closed.push(result.session_id);
        } else {
            report.failures.push(CloseFailure {
                session_id: result.session_id,
                error: result.error.unwrap_or_else(|| "Unknown close failure".to_owned()),
            });
        }
    }

    Ok(report)
}

#[derive(Debug, PartialEq, Eq)]
enum CloseTarget {
    All,
    Sessions(Vec<String>),
}

/// `all`, one session ID, or a comma-separated list of session IDs. Trims items and drops duplicates.
fn parse_close_target(raw: &str) -> Result<CloseTarget> {
    if raw.trim().is_empty() {
        bail!("Session ID must not be empty");
    }
    if raw.trim() == ALL_SESSIONS {
        return Ok(CloseTarget::All);
    }

    let mut session_ids: Vec<String> = Vec::new();
    for session_id in raw.split(',').map(str::trim) {
        if session_id.is_empty() {
            bail!("Session ID list contains an empty item");
        }
        if session_id == ALL_SESSIONS {
            bail!("Use `all` by itself, not inside a comma-separated session list");
        }
        if !session_ids.iter().any(|known| known == session_id) {
            session_ids.push(session_id.to_owned());
        }
    }

    Ok(CloseTarget::Sessions(session_ids))
}

fn format_close_report(report: &CloseReport) -> String {
    let mut lines = vec![format!("Worker: {}", report.worker)];
    if report.matched_sessions == 0 {
        lines.push("No open sessions.".to_owned());
    } else {
        lines.push(format!(
            "Requested: {} (matched {} session{})",
            report.requested,
            report.matched_sessions,
            if report.matched_sessions == 1 { "" } else { "s" }
        ));
    }
    lines.extend(
        report
            .closed
            .iter()
            .map(|session_id| format!("Closed session {session_id}")),
    );
    lines.extend(
        report
            .failures
            .iter()
            .map(|failure| format!("Failed to close {}: {}", failure.session_id, failure.error)),
    );

    lines.join("\n") + "\n"
}

fn summarize_failures(report: &CloseReport) -> String {
    match report.failures.as_slice() {
        [failure] => format!("Failed to close {}: {}", failure.session_id, failure.error),
        failures => format!(
            "Failed to close {} sessions: {}",
            failures.len(),
            failures
                .iter()
                .map(|failure| failure.session_id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{
        TimeDelta,
        TimeZone as _,
    };
    use cloudflare_worker_client::types::ActiveSessionRef;

    fn summary() -> SessionSummary {
        SessionSummary {
            active_sessions: 1.0,
            allowed_browser_acquisitions: 2.0,
            connected_sessions: 1.0,
            idle_sessions: 1.0,
            max_concurrent_sessions: 3.0,
            open_sessions: 2.0,
            time_until_next_allowed_browser_acquisition: 0.0,
        }
    }

    fn report(matched_sessions: usize, closed: &[&str], failures: &[(&str, &str)]) -> CloseReport {
        CloseReport {
            worker: "http://worker.test".to_owned(),
            requested: "all".to_owned(),
            matched_sessions,
            closed: closed.iter().map(|session_id| session_id.to_string()).collect(),
            failures: failures
                .iter()
                .map(|(session_id, error)| CloseFailure {
                    session_id: session_id.to_string(),
                    error: error.to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn worker_target_defaults_to_config() {
        let config = CloudflareConfig::default();

        let (base_url, timeout) = worker_target(&config, &WorkerArgs::default());

        assert_eq!(base_url, config.base_url.to_string());
        assert_eq!(timeout, Duration::from_secs(config.request_timeout_seconds));
    }

    #[test]
    fn worker_target_prefers_cli_flags_over_config() {
        let config = CloudflareConfig::default();
        let local = WorkerArgs {
            local: true,
            timeout: Some(5),
            ..Default::default()
        };
        let explicit = WorkerArgs {
            base_url: Some("http://worker.test".to_owned()),
            ..Default::default()
        };

        assert_eq!(
            worker_target(&config, &local),
            (LOCAL_WORKER_URL.to_owned(), Duration::from_secs(5))
        );
        assert_eq!(worker_target(&config, &explicit).0, "http://worker.test");
    }

    #[test]
    fn parses_close_targets() {
        assert_eq!(parse_close_target(" all ").unwrap(), CloseTarget::All);
        assert_eq!(
            parse_close_target(" a , b , a ").unwrap(),
            CloseTarget::Sessions(vec!["a".to_owned(), "b".to_owned()])
        );
    }

    #[test]
    fn rejects_empty_and_mixed_close_targets() {
        assert!(parse_close_target(" ").is_err());
        assert!(parse_close_target("a,,b").is_err());
        assert!(parse_close_target("a,all")
            .unwrap_err()
            .to_string()
            .contains("Use `all` by itself"));
    }

    #[test]
    fn formats_session_summary_and_entries() {
        let now = Utc.with_ymd_and_hms(2026, 8, 20, 12, 0, 0).unwrap();
        let started = (now - TimeDelta::seconds(3_725)).timestamp_millis() as f64;
        let connected_at = (now - TimeDelta::seconds(65)).timestamp_millis() as f64;
        let sessions = [
            Session {
                connection_id: Some("conn-1".to_owned()),
                connection_start_time: Some(connected_at),
                is_active: true,
                session_id: "session-1".to_owned(),
                start_time: started,
            },
            Session {
                connection_id: None,
                connection_start_time: None,
                is_active: false,
                session_id: "session-2".to_owned(),
                start_time: started,
            },
        ];

        let output = format_sessions("http://worker.test", &summary(), &sessions, now);

        assert!(output.starts_with("Worker: http://worker.test\nSummary\n"), "{output}");
        assert!(output.contains("  Open sessions:            2\n"), "{output}");
        assert!(output.contains("  Active session slots:     1 / 3\n"), "{output}");
        assert!(
            output.contains("  New acquisitions allowed: 2 (retry in 0 ms)\n"),
            "{output}"
        );
        assert!(output.contains("\nSessions\n- session-1\n  Started:      "), "{output}");
        assert!(output.contains("  Age:          1h 2m 5s\n"), "{output}");
        assert!(output.contains("  State:        connected\n"), "{output}");
        assert!(output.contains("  Active slot:  yes\n"), "{output}");
        assert!(output.contains("  Connection:   conn-1\n"), "{output}");
        assert!(output.contains("  Connected for: 1m 5s\n"), "{output}");
        assert!(output.contains("- session-2\n"), "{output}");
        assert!(output.contains("  State:        idle\n"), "{output}");
        assert!(output.contains("  Active slot:  no\n"), "{output}");
    }

    #[test]
    fn formats_empty_session_list() {
        let output = format_sessions("http://worker.test", &summary(), &[], Utc::now());

        assert!(output.ends_with("\n\nNo open sessions.\n"), "{output}");
    }

    #[test]
    fn formats_limits() {
        let limits = Limits {
            active_sessions: vec![ActiveSessionRef {
                id: "session-1".to_owned(),
            }],
            allowed_browser_acquisitions: 2.0,
            max_concurrent_sessions: 3.0,
            time_until_next_allowed_browser_acquisition: 1_500.0,
        };
        let docs = LimitsDocs {
            active_sessions: "Sessions holding a slot.".to_owned(),
            allowed_browser_acquisitions: "Acquisitions left.".to_owned(),
            max_concurrent_sessions: "Slot count.".to_owned(),
            time_until_next_allowed_browser_acquisition: "Wait time.".to_owned(),
        };

        assert_eq!(
            format_limits("http://worker.test", &limits, &docs),
            "Worker: http://worker.test\n\
             Limits\n\
             \x20 activeSessions\n\
             \x20   Sessions holding a slot.\n\
             \x20   - session-1\n\
             \x20 maxConcurrentSessions\n\
             \x20   Slot count.\n\
             \x20   - 3\n\
             \x20 allowedBrowserAcquisitions\n\
             \x20   Acquisitions left.\n\
             \x20   - 2\n\
             \x20 timeUntilNextAllowedBrowserAcquisition\n\
             \x20   Wait time.\n\
             \x20   - 1500 ms\n"
        );
    }

    #[test]
    fn formats_limits_without_active_sessions() {
        let limits = Limits {
            active_sessions: Vec::new(),
            allowed_browser_acquisitions: 2.0,
            max_concurrent_sessions: 3.0,
            time_until_next_allowed_browser_acquisition: 0.0,
        };
        let docs = LimitsDocs {
            active_sessions: String::new(),
            allowed_browser_acquisitions: String::new(),
            max_concurrent_sessions: String::new(),
            time_until_next_allowed_browser_acquisition: String::new(),
        };

        assert!(format_limits("http://worker.test", &limits, &docs).contains("\n    - none\n"));
    }

    #[test]
    fn formats_close_report_and_failure_summary() {
        let report = report(3, &["a"], &[("b", "boom"), ("c", "bang")]);

        assert_eq!(
            format_close_report(&report),
            "Worker: http://worker.test\nRequested: all (matched 3 sessions)\nClosed session a\nFailed to close b: boom\nFailed to close c: bang\n"
        );
        assert_eq!(summarize_failures(&report), "Failed to close 2 sessions: b, c");
    }

    #[test]
    fn formats_single_close_report_and_failure_summary() {
        let report = report(1, &[], &[("b", "boom")]);

        assert_eq!(
            format_close_report(&report),
            "Worker: http://worker.test\nRequested: all (matched 1 session)\nFailed to close b: boom\n"
        );
        assert_eq!(summarize_failures(&report), "Failed to close b: boom");
    }

    #[test]
    fn formats_close_report_without_matches() {
        assert_eq!(
            format_close_report(&report(0, &[], &[])),
            "Worker: http://worker.test\nNo open sessions.\n"
        );
    }

    #[test]
    fn formats_ages_without_zero_parts() {
        let now = Utc.with_ymd_and_hms(2026, 8, 20, 12, 0, 0).unwrap();
        let ms = |seconds: i64| (now - TimeDelta::seconds(seconds)).timestamp_millis() as f64;

        assert_eq!(format_age(ms(0), now), "0s");
        assert_eq!(format_age(ms(59), now), "59s");
        assert_eq!(format_age(ms(3_600), now), "1h");
        assert_eq!(format_age(ms(3_605), now), "1h 5s");
        assert_eq!(format_age(ms(-10), now), "0s");
    }

    #[test]
    fn formats_whole_numbers_without_fraction() {
        assert_eq!(number(3.0), "3");
        assert_eq!(number(0.0), "0");
        assert_eq!(number(2.5), "2.5");
    }
}
