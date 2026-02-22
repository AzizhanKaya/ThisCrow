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
                    close_session(user_id, state, 0).await;
                    break;
                }
            }
        }
    });

    let (user_res, groups_res, friends_res, incoming_res, outgoing_res) = tokio::join!(
        db::user::get_user(&state.pool, user_id),
        db::group::get_groups(&state.pool, user_id),
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
        friends,
        friend_requests: incoming,
        friend_requests_sent: outgoing,
        groups,
        dms,
        status: user::Status::Online,
    };

    let user_session = user::Session {
        id: user_id,
        connections: vec![user::Connection {
            reciver: tx,
            connection_id: 0,
        }],
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
        0,
    ));

    Ok(response)
}

fn add_connection(
    session: Session,
    user: &mut user::Session,
    msg_stream: MessageStream,
    state: &State,
) {
    let user_id = user.id;
    let connection_id = user
        .connections
        .iter()
        .map(|c| c.connection_id)
        .max()
        .unwrap_or(0)
        + 1;

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
                    close_session(user_id, state, connection_id).await;
                    break;
                }
            }
        }
    });

    let session_initialized = Message {
        to: user_id,
        data: Ack::Initialized(user.state.clone()),
        ..Default::default()
    };

    tx.send(Bytes::from(msgpack!(session_initialized)));

    user.connections.push(user::Connection {
        reciver: tx,
        connection_id,
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
                println!("Stream hatası: {:?}", e);
                break;
            }
            None => {
                println!("Stream kapandı");
                break;
            }
        }
    }
    close_session(user_id, state, connection_id).await;
}

async fn close_session(user_id: id, state: State, connection_id: usize) {
    if let Entry::Occupied(mut entry) = state.users.entry(user_id) {
        let user = entry.get_mut();

        if user.connections.len() <= 1 {
            entry.remove();

            if let Err(e) = db::user::update_last_seen(&state.pool, user_id, Utc::now()).await {
                warn!("Failed to update last_seen for {}: {:?}", user_id, e);
            }
        } else {
            user.connections
                .retain(|c| c.connection_id != connection_id);
        }
    }
}
