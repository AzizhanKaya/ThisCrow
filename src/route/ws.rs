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
use dashmap::Entry;
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

    let _user_lock = state.user_locks.write(user_id).await;

    if let Some(mut user) = state.users.get_mut(&user_id) {
        add_connection(session, user.value_mut(), msg_stream, &state);
        return Ok(response);
    }

    let connection_id = 1;

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
                    close_session(state, user_id, 1).await;
                    break;
                }
            }
        }
    });

    let (user_res, groups_res, friends_res, incoming_res, outgoing_res) = tokio::join!(
        db::user::get_user(&state.pool, user_id),
        db::user::get_groups(&state.pool, user_id),
        db::user::get_friends(&state.pool, user_id),
        db::user::friend_requests(&state.pool, user_id),
        db::user::outgoing_friend_requests(&state.pool, user_id)
    );

    let user = user_res
        .map_err(error::ErrorInternalServerError)?
        .ok_or(error::ErrorInternalServerError("User not found"))?;

    let groups: Vec<id> = groups_res.map_err(error::ErrorInternalServerError)?;
    let friends: HashSet<id> = friends_res
        .map_err(error::ErrorInternalServerError)?
        .into_iter()
        .collect();
    let incoming = incoming_res.map_err(error::ErrorInternalServerError)?;
    let outgoing = outgoing_res.map_err(error::ErrorInternalServerError)?;
    let dms: Vec<id> = state
        .messages
        .get_dms(user_id)
        .map_err(|e| error::ErrorInternalServerError(e))?;

    let user = user::State {
        id: user_id,
        version: id(0),
        username: user.username,
        name: user.name,
        avatar: user.avatar,
        groups,
        friends,
        friend_requests: incoming,
        friend_requests_sent: outgoing,
        dms,
        status: user::Status::Online,
        activities: vec![],
        voice: None,
    };

    let user_session = user::Session {
        connections: vec![user::Connection {
            id: connection_id,
            writer: tx,
        }],
        state: user.clone(),
    };

    state.users.insert(user_id, user_session);

    let session_initialized = Message {
        to: user_id,
        data: Ack::Initialized(Box::new(user)),
        ..Default::default()
    };

    dispatch::send_message(&state, session_initialized);

    actix_web::rt::spawn(handle_connection(
        session,
        user_id,
        msg_stream,
        state.clone(),
        connection_id,
    ));

    Ok(response)
}

fn add_connection(
    session: Session,
    user: &mut user::Session,
    msg_stream: MessageStream,
    state: &State,
) {
    let user_id = user.state.id;
    let connection_id = user.connections.iter().map(|c| c.id).max().unwrap_or(0) + 1;

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
                    close_session(state, user_id, connection_id).await;
                    break;
                }
            }
        }
    });

    let session_initialized = Message {
        to: user_id,
        data: Ack::Initialized(Box::new(user.state.clone())),
        ..Default::default()
    };

    tx.send(Bytes::from(msgpack!(session_initialized)));

    user.connections.push(user::Connection {
        id: connection_id,
        writer: tx,
    });

    actix_web::rt::spawn(handle_connection(
        session,
        user_id,
        msg_stream,
        state.clone(),
        connection_id,
    ));
}

async fn handle_connection(
    mut session: Session,
    user_id: id,
    msg_stream: MessageStream,
    state: State,
    connection_id: usize,
) {
    let mut stream = msg_stream.aggregate_continuations();

    loop {
        match stream.recv().await {
            Some(Ok(msg)) => match msg {
                WsMessage::Binary(bytes) => {
                    if let Err(e) =
                        dispatch::handle_bytes(user_id, &state, bytes, connection_id).await
                    {
                        warn!("Error handling message from user {}: {:#?}", user_id, e);
                        session
                            .binary(Bytes::from(msgpack!(Ack::Error(e.to_string()))))
                            .await;
                    }
                }
                WsMessage::Ping(ping) => {
                    session.pong(&ping).await;
                }
                WsMessage::Close(_) => break,
                _ => {
                    warn!("Unhandled WebSocket message type from user {}", user_id);
                }
            },
            Some(Err(e)) => {
                warn!("Stream hatası: {:?}", e);
                break;
            }
            None => {
                break;
            }
        }
    }
    close_session(state, user_id, connection_id).await;
}

async fn close_session(state: State, user_id: id, connection_id: usize) {
    let _user_lock = state.user_locks.write(user_id).await;
    let mut update_last_seen = false;

    if let Entry::Occupied(mut entry) = state.users.entry(user_id) {
        let user = entry.get_mut();

        if user.connections.len() <= 1 {
            entry.remove();
            update_last_seen = true;
        } else {
            user.connections.retain(|c| c.id != connection_id);
        }
    }

    if update_last_seen {
        if let Err(e) = db::user::update_last_seen(&state.pool, user_id, Utc::now()).await {
            warn!("Failed to update last_seen for {}: {:?}", user_id, e);
        }
    }
}
