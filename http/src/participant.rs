use crate::{
    error::AppError,
    router::AppState,
};
use axum::{
    extract::{
        ws::{
            Message,
            WebSocket,
            WebSocketUpgrade,
        },
        Query,
        State,
    },
    response::{
        IntoResponse,
        Response,
    },
};
use client_simulator_browser::participant::{
    messages::ParticipantMessage,
    transport_data::{
        ParticipantConfigQuery,
        ParticipantResponseMessage,
    },
    Participant,
};
use client_simulator_config::ParticipantConfig;
use eyre::{
    eyre,
    Result,
};
use futures::{
    sink::SinkExt,
    stream::{
        SplitSink,
        SplitStream,
        StreamExt,
    },
};

#[derive(serde::Deserialize)]
pub struct QueryPayload {
    payload: String,
}

pub async fn handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(query): Query<QueryPayload>,
) -> Response {
    let participant_config_query = match ParticipantConfigQuery::try_from(query.payload) {
        Ok(v) => v,
        Err(e) => return AppError::ParticipantConfig(e).into_response(),
    };

    ws.on_upgrade(move |socket| handle_socket(socket, state, participant_config_query))
}

async fn handle_socket(socket: WebSocket, state: AppState, query: ParticipantConfigQuery) {
    info!("New WebSocket connection");

    let (mut sender, receiver) = socket.split();

    if let Err(e) = handle_socket_inner(&mut sender, receiver, state, query).await {
        error!("Error handling WebSocket connection: {}", e);
        sender
            .send(AppError::Socket(e).into_message())
            .await
            .unwrap_or_else(|e| error!("Failed to send error message: {}", e));
    }
}

async fn handle_socket_inner(
    sender: &mut SplitSink<WebSocket, Message>,
    mut receiver: SplitStream<WebSocket>,
    state: AppState,
    query: ParticipantConfigQuery,
) -> Result<()> {
    let (config, borrowed_cookie) = query.into_config_and_cookie(&state.config, state.cookie_manager.clone());
    let borrowed_cookie = borrowed_cookie.ok_or_else(|| eyre!("Cannot connect to session without a cookie"))?;

    let participant_config = ParticipantConfig::new(&config, Some(borrowed_cookie.username().to_string()))?;
    debug!("Creating a new participant with config: {:?}", config);

    let (mut participant, mut participant_receiver) =
        Participant::with_participant_config(participant_config, Some(borrowed_cookie), state.cookie_manager)?;

    let mut rx = participant.state.clone();

    loop {
        tokio::select! {
            Some(message) = receiver.next() => {
                match message {
                    Ok(Message::Text(text)) => {
                        let message = serde_json::from_str::<ParticipantMessage>(&text)?;

                        if let ParticipantMessage::Close = message {
                            participant.close().await;
                            sender.send(Message::Close(None)).await?;
                            return Ok(());
                        }

                        participant.send_message(message);
                    }
                    Ok(Message::Binary(_)) => {
                        info!("Received binary message");
                    }
                    Ok(Message::Close(_)) => {
                        info!("WebSocket connection closed by client");
                        participant.close().await;
                        sender.send(Message::Close(None)).await?;
                        return Ok(());
                    }
                    Ok(Message::Ping(ping)) => {
                        info!("Received ping: {:?}", ping);
                        sender.send(Message::Pong(ping)).await?;
                    }
                    Ok(Message::Pong(pong)) => {
                        info!("Received pong: {:?}", pong);
                    }
                    Err(e) => {
                        sender.send(AppError::Socket(e.into()).into_message()).await?;
                        participant.close().await;
                        return Ok(());
                    }
                }
            },
            message = participant_receiver.recv() => {
                match message {
                    Some(msg) => {
                        info!("Received participant message: {:?}", msg);
                        let json = ParticipantResponseMessage::new(participant.state.borrow().clone(), msg).to_string();
                        sender.send(Message::Text(json.into())).await?;
                    }
                    None => {
                        info!("Participant channel closed");
                        participant.close().await;
                        return Ok(());
                    }
                }
            },
            Ok(_) = rx.changed() => {
                let state = participant.state.borrow_and_update().clone();
                debug!("Participant state changed: {:?}", &state);
                let json = ParticipantResponseMessage::from_state(state).to_string();
                sender.send(Message::Text(json.into())).await?;
            },
        };
    }
}
