use axum::serve;
use axum_server::tls_rustls::RustlsConfig;
use clap::Parser;
use client_simulator_browser::auth::HyperSessionCookieStash;
use client_simulator_config::Config;
use client_simulator_http::router::create_router;
use color_eyre::Result;
use std::{
    net::SocketAddr,
    path::PathBuf,
};
use tokio::net::TcpListener;
use tracing_subscriber::{
    fmt,
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter,
    Layer,
};

#[derive(Debug, clap::Args)]
#[group(required = true, multiple = true)]
struct HttpArgs {
    /// IP address to listen on.
    #[arg(long, env = "CLIENT_SIMULATOR_HTTP_ADDRESS", default_value = "127.0.0.1:8081")]
    http_listen_address: SocketAddr,

    /// Should the HTTP server terminate TLS connections?
    #[arg(long, action, env = "CLIENT_SIMULATOR_HTTP_TLS")]
    tls: bool,
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[clap(flatten)]
    http: HttpArgs,

    /// Path to the X.509 public key certificate in DER encoding.
    #[arg(long)]
    certificate: PathBuf,

    /// Path to the private key for the X.509 certificate in DER encoding.
    #[arg(long)]
    private_key: PathBuf,
}

fn init_logging() {
    color_eyre::install().expect("color_eyre init");

    tracing_subscriber::registry()
        .with(fmt::layer().with_filter(EnvFilter::from_default_env()))
        .with(tracing_error::ErrorLayer::default())
        .init();
}

async fn start_server(args: Args) -> Result<()> {
    let config = Config::default();
    let cookie_stash = HyperSessionCookieStash::load_from_data_dir(config.data_dir());
    let app = create_router(config, cookie_stash.into());

    tracing::info!("listening on {}", args.http.http_listen_address);

    if args.http.tls {
        let rustls_config = RustlsConfig::from_pem_file(args.certificate, args.private_key).await?;
        axum_server::bind_rustls(args.http.http_listen_address, rustls_config)
            .serve(app.into_make_service())
            .await?;
    } else {
        let listener = TcpListener::bind(args.http.http_listen_address).await?;
        serve(listener, app.into_make_service()).await?;
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();
    start_server(Args::parse()).await
}
