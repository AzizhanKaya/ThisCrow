use crate::State;
use crate::TOKIO_RT;
use crate::db;
use crate::id::id;
use crate::message::Ack;
use crate::message::Message;
use crate::message::dispatch;
use crate::middleware;
use crate::msgpack;
use crate::state::user;
use anyhow::Result;
use bytes::Bytes;
use chrono::Utc;
use dashmap::Entry;
use flume::Receiver;
use flume::unbounded;
use log::{info, warn};
use monoio::io::sink::Sink;
use monoio::io::stream::Stream;
use monoio::net::{TcpListener, TcpStream};
use monoio::time::Duration;
use monoio::time::timeout;
use monoio_tungstenite::Message as WsMessage;
use monoio_tungstenite::WebSocket;
use monoio_tungstenite::protocol::WebSocketConfig;
use once_cell::sync::Lazy;
use socket2::{Domain, Protocol, Socket, Type};
use std::collections::HashSet;
use std::net::{Ipv4Addr, SocketAddr};
use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::http;

pub async fn listen(port: u16, state: State) -> Result<()> {
    let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))?;
    socket.set_reuse_port(true)?;
    socket.set_reuse_address(true)?;

    let addr: SocketAddr = SocketAddr::new(Ipv4Addr::new(127, 0, 0, 1).into(), port);
    socket.bind(&addr.into())?;
    socket.listen(1024)?;

    let std_listener: std::net::TcpListener = socket.into();
    std_listener.set_nonblocking(true)?;

    let listener = TcpListener::from_std(std_listener)?;
    info!("Monoio WebSocket server listening on {}", port);

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let state = state.clone();
                monoio::spawn(async move {
                    ws_handshake(stream, state).await;
                });
            }
            Err(e) => {
                warn!("Accept error: {:?}", e);
            }
        }
    }
}

static WS_CONFIG: Lazy<WebSocketConfig> = Lazy::new(|| {
    WebSocketConfig::default()
        .max_frame_size(Some(1024))
        .max_message_size(Some(1024))
        .write_buffer_size(1024)
        .accept_unmasked_frames(true)
});

async fn ws_handshake(stream: TcpStream, state: State) {
    let mut user_id: id = Default::default();

    let callback = |req: &Request, response: Response| -> Result<Response, Box<ErrorResponse>> {
        let user = req
            .headers()
            .get(http::header::COOKIE)
            .and_then(|header| header.to_str().ok())
            .and_then(|cookie_str| {
                cookie_str.split(';').find_map(|pair| {
                    let (key, value) = pair.trim().split_once('=')?;
                    if key == "session" { Some(value) } else { None }
                })
            })
            .and_then(middleware::verify_jwt);

        if let Some(user) = user {
            user_id = user.id;
            return Ok(response);
        }

        let reject = http::Response::builder()
            .status(http::StatusCode::UNAUTHORIZED)
            .body(Some("Unauthorized".to_string()))
            .unwrap();

        Err(Box::new(reject))
    };

    match monoio_tungstenite::accept_hdr_with_config(stream, callback, Some(*WS_CONFIG)).await {
        Ok(stream) => {
            initialize_session(stream, user_id, state).await;
        }
        Err(e) => {
            warn!("WebSocket handshake failed: {:?}", e);
        }
    }
}

async fn initialize_session(
    stream: WebSocket<TcpStream>,
    user_id: id,
    state: State,
) -> anyhow::Result<()> {
    let _user_lock = state.user_locks.write(user_id).await;

    if let Some(mut user) = state.users.get_mut(&user_id) {
        add_connection(stream, user.value_mut(), &state).await;
        return Ok(());
    }

    let pool = state.pool.clone();

    let db_tasks = TOKIO_RT.spawn(async move {
        tokio::join!(
            db::user::get_user(&pool, user_id),
            db::user::get_groups(&pool, user_id),
            db::user::get_friends(&pool, user_id),
            db::user::friend_requests(&pool, user_id),
            db::user::outgoing_friend_requests(&pool, user_id)
        )
    });

    let (user_res, groups_res, friends_res, incoming_res, outgoing_res) = db_tasks.await?;

    let user = user_res?;
    let groups: Vec<id> = groups_res?;
    let friends: HashSet<id> = friends_res?.into_iter().collect();
    let incoming = incoming_res?;
    let outgoing = outgoing_res?;
    let dms: Vec<id> = state.messages.get_dms(user_id)?;

    let user_state = user::State {
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

    let session_initialized = Message {
        to: user_id,
        data: Ack::Initialized(Box::new(user_state.clone())),
        ..Default::default()
    };
    let connection_id = 1;
    let (tx, rx) = unbounded::<Bytes>();

    let _ = tx.send(Bytes::from(msgpack!(session_initialized)));

    let user_session = user::Session {
        connections: vec![user::Connection {
            id: connection_id,
            writer: tx,
        }],
        state: user_state,
    };

    state.users.insert(user_id, user_session);

    monoio::spawn(handle_connection(
        stream,
        rx,
        user_id,
        connection_id,
        state.clone(),
    ));

    Ok(())
}

async fn add_connection(
    stream: monoio_tungstenite::WebSocket<TcpStream>,
    user: &mut user::Session,
    state: &State,
) {
    let user_id = user.state.id;
    let connection_id = user.connections.iter().map(|c| c.id).max().unwrap_or(0) + 1;

    let (tx, rx) = unbounded::<Bytes>();

    let session_initialized = Message {
        to: user_id,
        data: Ack::Initialized(Box::new(user.state.clone())),
        ..Default::default()
    };

    let _ = tx.send(Bytes::from(msgpack!(session_initialized)));

    user.connections.push(user::Connection {
        id: connection_id,
        writer: tx,
    });

    monoio::spawn(handle_connection(
        stream,
        rx,
        user_id,
        connection_id,
        state.clone(),
    ));
}

async fn handle_connection(
    mut stream: WebSocket<TcpStream>,
    rx: Receiver<Bytes>,
    user_id: id,
    connection_id: usize,
    state: State,
) {
    loop {
        monoio::select! {
            incoming = stream.next() => {
                 match incoming {
                    Some(Ok(message)) => {
                        if handle_incoming(message, &mut stream, user_id, connection_id, &state).await.is_err() {
                            break;
                        }
                    }
                    Some(Err(e)) => {
                        log::warn!("WebSocket read error ({user_id}): {:?}", e);
                        break;
                    }
                    None => break,
                };
            },

            outgoing = rx.recv_async() => {
                match outgoing {
                    Ok(outgoing) => {
                        let queue_len = rx.len();
                        if handle_outgoing(outgoing, &mut stream, queue_len, user_id).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    }

    log::info!("user disconnected: {user_id}");
    disconnect(user_id, connection_id, state).await;
}

#[inline]
async fn handle_incoming(
    message: WsMessage,
    stream: &mut WebSocket<TcpStream>,
    user_id: id,
    connection_id: usize,
    state: &State,
) -> Result<(), ()> {
    match message {
        WsMessage::Binary(bytes) => {
            if let Err(e) = dispatch::handle_bytes(bytes, user_id, connection_id, state).await {
                log::warn!("Message error for user: {e:?}");
            }
        }
        WsMessage::Ping(ping) => {
            if let Err(e) = stream.send(WsMessage::Pong(ping)).await {
                log::warn!("Failed to send pong to {user_id}: {:?}", e);
                return Err(());
            }
        }
        WsMessage::Close(_) => return Err(()),
        _ => log::warn!("Unhandled websocket message type from {user_id}: {message:?}"),
    }

    Ok(())
}

#[inline]
async fn handle_outgoing(
    message: Bytes,
    stream: &mut WebSocket<TcpStream>,
    queue_len: usize,
    user_id: id,
) -> Result<(), ()> {
    if queue_len > 100 {
        log::warn!("Message queue full (>100) for {user_id}, disconnecting.");
        return Err(());
    }
    let send_future = stream.send(WsMessage::Binary(message));
    if timeout(Duration::from_secs(5), send_future).await.is_err() {
        log::warn!("Send timeout or error for user: {user_id}");
        return Err(());
    }

    Ok(())
}

async fn disconnect(user_id: id, connection_id: usize, state: State) {
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
        let pool = state.pool.clone();
        TOKIO_RT.spawn(async move {
            if let Err(e) = db::user::update_last_seen(&pool, user_id, Utc::now()).await {
                warn!("Failed to update last_seen for {}: {:?}", user_id, e);
            }
        });
    }
}
