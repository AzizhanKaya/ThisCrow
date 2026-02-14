use crate::State;
use crate::db;
use crate::id::id;
use crate::message::Ack;
use crate::message::Message;
use crate::message::dispatch;
use crate::middleware::JwtUser;
use crate::msgpack;
use crate::state::user;
use actix_web::error;
use actix_web::web;
use actix_web::{Error, HttpRequest, HttpResponse};
use actix_ws::AggregatedMessage as WsMessage;
use actix_ws::MessageStream;
use actix_ws::Session;
use anyhow::Result;
use bytes::Bytes;
use chrono::Utc;

use log::warn;
use std::collections::HashSet;
use tokio::sync::mpsc;
use tokio::time::{Duration, timeout};

pub async fn ws(
    req: HttpRequest,
    stream: web::Payload,
    state: State,
    user: web::ReqData<JwtUser>,
) -> Result<HttpResponse, Error> {
    let (response, session, msg_stream) = actix_ws::handle(&req, stream)?;

    let user_id = user.id;

    let (tx, mut rx) = mpsc::unbounded_channel::<Bytes>();

    actix_web::rt::spawn({
        let mut session = session.clone();
        let state = state.clone();
        async move {
            while let Some(msg) = rx.recv().await {
                if timeout(Duration::from_secs(5), session.binary(msg))
                    .await
                    .is_err()
                    || rx.len() > 100
                {
                    close_session(user_id, state).await;
                    break;
                }
            }
        }
    });

    let _user_lock = state.user_locks.read(user_id).await;

    let friends: HashSet<id> = db::user::get_friends(&state.pool, user_id)
        .await
        .map_err(|e| error::ErrorInternalServerError(e))?
        .into_iter()
        .collect();

    let groups: Vec<id> = db::group::get_groups(&state.pool, user_id)
        .await
        .map_err(|e| error::ErrorInternalServerError(e))?;

    let dms: Vec<id> = state
        .messages
        .get_dms(user_id)
        .map_err(|e| error::ErrorInternalServerError(e))?;

    let user = db::user::get_user(&state.pool, user_id)
        .await
        .ok_or(error::ErrorUnauthorized(""))?;

    let incoming = db::user::friend_requests(&state.pool, user_id)
        .await
        .map_err(|e| error::ErrorInternalServerError(e))?;

    let outgoing = db::user::outgoing_friend_requests(&state.pool, user_id)
        .await
        .map_err(|e| error::ErrorInternalServerError(e))?;

    let user = user::State {
        id: user_id,
        version: id(0),
        username: user.username,
        name: user.name,
        avatar: user.avatar,
        friends,
        friend_requests: incoming,
        friend_requests_sent: outgoing,
        groups,
        dms,
        status: user::Status::Online,
    };

    let user_session = user::Session {
        id: user_id,
        tx: tx,
        state: user.clone(),
    };

    state.users.insert(user_id, user_session);

    let session_initialized = Message {
        to: user_id,
        data: Ack::Initialized(user),
        ..Default::default()
    };

    dispatch::send_message(&state, session_initialized);

    actix_web::rt::spawn(handle_connection(
        session,
        user_id,
        msg_stream,
        state.clone(),
    ));

    Ok(response)
}

async fn handle_connection(
    mut session: Session,
    user_id: id,
    msg_stream: MessageStream,
    state: State,
) {
    let mut stream = msg_stream.aggregate_continuations();
    while let Some(Ok(msg)) = stream.recv().await {
        match msg {
            WsMessage::Binary(bytes) => {
                if let Err(e) = dispatch::handle_bytes(user_id, &state, bytes).await {
                    warn!("Error handling message from user {}: {:#?}", user_id, e);
                    session
                        .binary(Bytes::from(msgpack!(Ack::Error(e.to_string()))))
                        .await;
                }
            }
            WsMessage::Close(_) => {
                close_session(user_id, state).await;
                break;
            }
            WsMessage::Ping(ping) => {
                session.pong(&ping).await;
            }
            _ => {
                warn!("Unhandled WebSocket message type from user {}", user_id);
            }
        }
    }
}

async fn close_session(user_id: id, state: State) {
    state.users.remove(&user_id);

    if let Err(e) = db::user::update_last_seen(&state.pool, user_id, Utc::now()).await {
        warn!("Failed to update last_seen for {}: {:?}", user_id, e);
    }
}
