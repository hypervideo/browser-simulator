mod aws;
mod errors;
mod headless;

use clap::{
    Parser,
    Subcommand,
};
use client_simulator_config::TuiArgs;
use client_simulator_tui::start_tui;
use eyre::{
    Context as _,
    OptionExt as _,
};
use tracing_subscriber::{
    filter::LevelFilter,
    fmt,
    prelude::*,
    registry,
    EnvFilter,
};

const DEFAULT_LOGGING_DIRECTIVE: &str = "info";
const RUST_LOG_ENV: &str = "RUST_LOG";

#[derive(Copy, Clone, Debug, Eq, PartialEq, clap::ValueEnum)]
pub enum Logging {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl Logging {
    fn as_filter(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }
}

fn logging_filter(logging: Option<Logging>, rust_log: Option<&str>) -> EnvFilter {
    let directive = logging
        .map(Logging::as_filter)
        .or(rust_log)
        .unwrap_or(DEFAULT_LOGGING_DIRECTIVE);

    EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .parse_lossy(directive)
}

fn logging_filter_from_env(logging: Option<Logging>) -> EnvFilter {
    let rust_log = std::env::var(RUST_LOG_ENV).ok();
    logging_filter(logging, rust_log.as_deref())
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct CliArgs {
    /// Tracing filter level. If absent, RUST_LOG is used.
    #[clap(long, value_enum, value_name = "LEVEL")]
    pub logging: Option<Logging>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Start the TUI application
    Tui(TuiArgs),
    /// Start simulator participants without the TUI
    Headless(headless::HeadlessArgs),
    /// Connect to the hyper server to get a hyper session cookie
    Cookie(CookieArgs),
    /// Manage AWS Device Farm Test Grid sessions
    Aws(aws::AwsArgs),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_headless_command_with_repeated_participants() {
        let args = CliArgs::parse_from([
            "hyper-client-simulator",
            "--logging",
            "debug",
            "headless",
            "--url",
            "https://latest.dev.hyper.video/F27-T5F-DXY",
            "--participant",
            r#"{"audio_enabled": false, "backend": "local"}"#,
            "--participant",
            r#"{"audio_enabled": true, "backend": "cloudflare"}"#,
        ]);

        match args.command {
            Some(Command::Headless(headless)) => {
                assert_eq!(args.logging, Some(Logging::Debug));
                assert_eq!(
                    headless.url.as_ref().map(url::Url::as_str),
                    Some("https://latest.dev.hyper.video/F27-T5F-DXY")
                );
                assert_eq!(headless.participants.len(), 2);
            }
            other => panic!("expected headless command, got {other:?}"),
        }
    }

    #[test]
    fn parses_logging_levels() {
        for (value, logging) in [
            ("error", Logging::Error),
            ("warn", Logging::Warn),
            ("info", Logging::Info),
            ("debug", Logging::Debug),
            ("trace", Logging::Trace),
        ] {
            let args = CliArgs::parse_from(["hyper-client-simulator", "--logging", value, "headless"]);

            assert_eq!(args.logging, Some(logging));
        }
    }

    #[test]
    fn leaves_logging_empty_when_not_provided() {
        let args = CliArgs::parse_from(["hyper-client-simulator", "headless"]);

        assert_eq!(args.logging, None);
    }

    #[test]
    fn rejects_unknown_logging_level() {
        let err = CliArgs::try_parse_from(["hyper-client-simulator", "--logging", "verbose", "headless"])
            .expect_err("unknown logging level should fail");

        assert_eq!(err.kind(), clap::error::ErrorKind::InvalidValue);
    }

    #[test]
    fn uses_cli_logging_for_filter_when_present() {
        let filter = logging_filter(Some(Logging::Trace), Some("warn"));

        assert_eq!(filter.to_string(), "trace");
    }

    #[test]
    fn uses_rust_log_for_filter_when_cli_logging_is_absent() {
        let filter = logging_filter(None, Some("warn,client_simulator_browser=debug"));

        let filter = filter.to_string();
        assert!(filter.contains("warn"));
        assert!(filter.contains("client_simulator_browser=debug"));
    }

    #[test]
    fn parses_aws_list_sessions_json() {
        let args = CliArgs::parse_from(["hyper-client-simulator", "aws", "list-sessions", "--json"]);
        match args.command {
            Some(Command::Aws(aws::AwsArgs {
                command: aws::AwsCommand::ListSessions(args),
            })) => {
                assert!(args.json);
                assert_eq!(args.status, aws::ListSessionsStatus::Active);
            }
            other => panic!("expected aws list-sessions, got {other:?}"),
        }
    }

    #[test]
    fn parses_aws_list_sessions_closed_status() {
        let args = CliArgs::parse_from(["hyper-client-simulator", "aws", "list-sessions", "--status", "closed"]);
        match args.command {
            Some(Command::Aws(aws::AwsArgs {
                command: aws::AwsCommand::ListSessions(args),
            })) => {
                assert_eq!(args.status, aws::ListSessionsStatus::Closed);
            }
            other => panic!("expected aws list-sessions, got {other:?}"),
        }
    }

    #[test]
    fn parses_aws_list_sessions_all_status() {
        let args = CliArgs::parse_from(["hyper-client-simulator", "aws", "list-sessions", "--status", "all"]);
        match args.command {
            Some(Command::Aws(aws::AwsArgs {
                command: aws::AwsCommand::ListSessions(args),
            })) => {
                assert_eq!(args.status, aws::ListSessionsStatus::All);
            }
            other => panic!("expected aws list-sessions, got {other:?}"),
        }
    }

    #[test]
    fn parses_aws_list_sessions_since_date() {
        let args = CliArgs::parse_from([
            "hyper-client-simulator",
            "aws",
            "list-sessions",
            "--since",
            "2026-06-04",
        ]);
        match args.command {
            Some(Command::Aws(aws::AwsArgs {
                command: aws::AwsCommand::ListSessions(args),
            })) => {
                assert!(args.since.is_some());
            }
            other => panic!("expected aws list-sessions, got {other:?}"),
        }
    }

    #[test]
    fn parses_aws_list_sessions_since_date_time() {
        let args = CliArgs::parse_from([
            "hyper-client-simulator",
            "aws",
            "list-sessions",
            "--since",
            "2026-06-04 10:11",
        ]);
        match args.command {
            Some(Command::Aws(aws::AwsArgs {
                command: aws::AwsCommand::ListSessions(args),
            })) => {
                assert!(args.since.is_some());
            }
            other => panic!("expected aws list-sessions, got {other:?}"),
        }
    }

    #[test]
    fn parses_aws_list_sessions_since_human_duration() {
        let args = CliArgs::parse_from(["hyper-client-simulator", "aws", "list-sessions", "--since", "1 day"]);
        match args.command {
            Some(Command::Aws(aws::AwsArgs {
                command: aws::AwsCommand::ListSessions(args),
            })) => {
                assert!(args.since.is_some());
            }
            other => panic!("expected aws list-sessions, got {other:?}"),
        }
    }

    #[test]
    fn parses_aws_close_sessions_with_comma_separated_ids() {
        let args = CliArgs::parse_from(["hyper-client-simulator", "aws", "close-sessions", "a,b,c"]);
        match args.command {
            Some(Command::Aws(aws::AwsArgs {
                command: aws::AwsCommand::CloseSessions(args),
            })) => {
                assert_eq!(args.session_ids, vec!["a", "b", "c"]);
            }
            other => panic!("expected aws close-sessions, got {other:?}"),
        }
    }

    #[test]
    fn parses_aws_close_sessions_without_ids() {
        let args = CliArgs::parse_from(["hyper-client-simulator", "aws", "close-sessions"]);
        match args.command {
            Some(Command::Aws(aws::AwsArgs {
                command: aws::AwsCommand::CloseSessions(args),
            })) => {
                assert!(args.session_ids.is_empty());
            }
            other => panic!("expected aws close-sessions, got {other:?}"),
        }
    }

    #[test]
    fn parses_aws_setup_auth() {
        let args = CliArgs::parse_from(["hyper-client-simulator", "aws", "setup-auth"]);
        match args.command {
            Some(Command::Aws(aws::AwsArgs {
                command: aws::AwsCommand::SetupAuth,
            })) => {}
            other => panic!("expected aws setup-auth, got {other:?}"),
        }
    }
}

#[derive(clap::Args, Debug, Clone)]
pub struct CookieArgs {
    /// Base URL of the hyper server
    #[clap(long = "url", value_name = "URL", default_value = "http://localhost:8081")]
    pub base_url: url::Url,

    /// Username for the hyper session
    #[clap(long, value_name = "USERNAME", default_value = "browser-simulator user")]
    pub user: String,
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    errors::init()?;

    let CliArgs { command, logging } = CliArgs::parse();

    match command {
        None => {
            let args = TuiArgs::default();
            start_tui(args, logging_filter_from_env(logging)).await
        }
        Some(Command::Tui(args)) => start_tui(args, logging_filter_from_env(logging)).await,
        Some(Command::Headless(args)) => {
            let code = headless::run(args, logging_filter_from_env(logging)).await?;
            std::process::exit(code);
        }
        Some(Command::Cookie(args)) => run_cookie(args, logging_filter_from_env(logging)).await,
        Some(Command::Aws(args)) => aws::run(args, logging_filter_from_env(logging)).await,
    }
}

async fn run_cookie(CookieArgs { base_url, user }: CookieArgs, filter: EnvFilter) -> eyre::Result<()> {
    registry()
        .with(
            fmt::layer()
                .with_writer(std::io::stderr)
                .with_span_events(fmt::format::FmtSpan::CLOSE)
                .with_filter(filter),
        )
        .with(tracing_error::ErrorLayer::default())
        .init();

    let domain = base_url
        .host_str()
        .ok_or_eyre("Base URL must have a valid host")?
        .to_string();
    let config = client_simulator_config::Config::new(Default::default()).context("Failed to create config")?;
    let participants_store = client_simulator_browser::participant::ParticipantStore::new(config.data_dir());
    let cookie = participants_store
        .cookies()
        .give_or_fetch_cookie(base_url, user)
        .await
        .context("Failed to fetch or give cookie")?;
    let cookie = cookie
        .as_browser_cookie_for(&domain)
        .context("Failed to convert cookie for browser")?;
    let json = serde_json::to_string(&cookie).context("Failed to serialize cookie to JSON")?;

    println!("{json}");

    Ok(())
}
