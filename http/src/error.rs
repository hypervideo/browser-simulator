use axum::{
    extract::ws::Message,
    http::StatusCode,
    response::{
        IntoResponse,
        Response,
    },
};

#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error("The configuration cannot be used to join the participant: {0}")]
    ParticipantConfig(eyre::Report),
    #[error("Handling the websocket connection failed: {0}")]
    Socket(eyre::Report),
}

impl AppError {
    pub fn into_message(self) -> Message {
        Message::Text(serde_json::json!({ "error": self.to_string() }).to_string().into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({ "error": self.to_string() })),
        )
            .into_response()
    }
}
