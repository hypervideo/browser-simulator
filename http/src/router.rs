use crate::participant::handler;
use axum::{
    routing::get,
    Router,
};
use client_simulator_browser::auth::HyperSessionCookieManger;
use client_simulator_config::Config;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub cookie_manager: HyperSessionCookieManger,
}

pub fn create_router(config: Config, cookie_manager: HyperSessionCookieManger) -> Router {
    let state = AppState { config, cookie_manager };

    Router::new()
        .route("/healthz", get(healthz))
        .route("/", get(handler))
        .with_state(state)
}

async fn healthz() -> &'static str {
    "Hello!"
}
