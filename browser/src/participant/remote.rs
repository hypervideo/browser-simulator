use crate::{
    auth::{
        BorrowedCookie,
        HyperSessionCookieManger,
    },
    participant::{
        messages::ParticipantMessage,
        transport_data::{
            ParticipantConfigQuery,
            ParticipantResponseMessage,
        },
        ParticipantState,
    },
};
use eyre::{
    eyre,
    Result,
};
use futures::{
    SinkExt,
    StreamExt,
};
use tokio::{
    sync::{
        mpsc::UnboundedReceiver,
        watch,
    },
    task::JoinHandle,
};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        protocol::Message,
        Error,
    },
};

pub async fn spawn_remote(
    mut receiver: UnboundedReceiver<ParticipantMessage>,
    state_sender: watch::Sender<ParticipantState>,
    mut query: ParticipantConfigQuery,
    cookie: Option<BorrowedCookie>,
    cookie_manager: HyperSessionCookieManger,
) -> Result<()> {
    let maybe_new_cookie = query.ensure_cookie(cookie_manager).await?;

    info!("Connecting to WebSocket: {}", query.remote_url);

    let (ws_stream, response) = match connect_async(query.into_url()?.to_string()).await {
        Ok(result) => result,
        Err(e) => {
            // Check if the error contains an HTTP response
            if let Error::Http(ref response) = e {
                let status = response.status();
                let headers = response.headers();
                let body = response.body().clone().unwrap_or_default();
                let body_str = String::from_utf8(body.clone()).unwrap_or_else(|_| format!("Non-UTF8 body: {:?}", body));
                error!(
                    "WebSocket connection failed: status={}, headers={:?}, body={}",
                    status, headers, body_str
                );
            } else {
                error!("WebSocket connection failed: {}", e);
            }
            return Err(eyre!("Failed to connect to WebSocket: {}", e));
        }
    };

    info!("WebSocket connected: {:?}", response);

    let (mut outgoing, mut incoming) = ws_stream.split();

    // Handle outgoing messages
    let send_task: JoinHandle<()> = tokio::spawn(async move {
        info!("Starting send task");
        while let Some(message) = receiver.recv().await {
            debug!("Sending message: {:?}", message);
            match serde_json::to_string(&message) {
                Ok(text) => {
                    if let Err(e) = outgoing.send(Message::Text(text.into())).await {
                        error!("Error sending message: {}", e);
                        break;
                    }
                }
                Err(e) => {
                    error!("Error serializing message: {}", e);
                }
            }
        }
        info!("Send task completed");
    });

    // Handle incoming messages
    let recv_task: JoinHandle<()> = tokio::spawn(async move {
        info!("Starting receive task");
        while let Some(result) = incoming.next().await {
            match result {
                Ok(msg) => match msg {
                    Message::Text(text) => {
                        trace!("Received message, will try to deserialize it: {}", text);
                        let message_result: Result<ParticipantResponseMessage, _> = serde_json::from_str(&text);
                        match message_result {
                            Ok(message) => {
                                debug!("Received message: {:?}", message);

                                state_sender.send_modify(|state| {
                                    *state = message.state;
                                });

                                if let Some(log) = message.log {
                                    log.write();
                                }
                            }
                            Err(e) => {
                                error!("Error deserializing message: {}", e);
                            }
                        }
                    }
                    Message::Binary(_) => {
                        trace!("Received binary message");
                    }
                    Message::Ping(ping) => {
                        debug!("Received ping: {:?}", ping);
                    }
                    Message::Pong(pong) => {
                        debug!("Received pong: {:?}", pong);
                    }
                    Message::Close(_) => {
                        debug!("Received close message");
                        break;
                    }
                    Message::Frame(_) => {
                        trace!("Received frame message");
                    }
                },
                Err(e) => {
                    error!("Error receiving message: {}", e);
                    break;
                }
            }
        }
        info!("Receive task completed");
    });

    tokio::select! {
        result = send_task => {
            result?;
        }
        result = recv_task => {
            result?;
        }
    };

    // Moving the both possible cookies here to have them live as long as the remote participant is alive
    // to avoid giving the same cookie to another participant.
    let _ = (cookie, maybe_new_cookie);

    Ok(())
}
