use crate::State;
use crate::auth::JwtUser;
use crate::db;
use crate::models::Event::{ChangeState, ExitChannel, JoinChannel};
use crate::models::{Event, Message, MessageType, State as UserState, User, id};
use actix_web::web;
use actix_web::{Error, HttpRequest, HttpResponse};
use actix_ws::MessageStream;
use actix_ws::{Message as WsMessage, Session};
use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use dashmap::DashSet;
use futures_util::StreamExt;
use log::warn;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

pub async fn ws(
    req: HttpRequest,
    stream: web::Payload,
    state: State,
    user: web::ReqData<JwtUser>,
) -> Result<HttpResponse, Error> {
    let (response, session, msg_stream) = actix_ws::handle(&req, stream)?;

    let user_id = user.id;
    let username = user.username.clone();

    let user = User {
        username: username.clone(),
        state: UserState::Online,
        ws_conn: session.clone(),
        voice_chat: None,
    };

    state.users.insert(user_id, user);

    actix_web::rt::spawn(handle_connection(
        user_id,
        session,
        msg_stream,
        state.clone(),
    ));

    Ok(response)
}

async fn handle_connection(
    user_id: id,
    mut session: actix_ws::Session,
    mut msg_stream: MessageStream,
    state: State,
) {
    while let Some(Ok(msg)) = msg_stream.next().await {
        match msg {
            WsMessage::Text(text) => {
                if let Err(e) = handle_text(user_id, &state, text.to_string()).await {
                    warn!("Error from user: {}\n {:#?}", user_id, e);
                }
            }
            WsMessage::Close(_) => {
                break;
            }
            WsMessage::Ping(ping) => {
                let _ = session.pong(&ping).await;
            }
            WsMessage::Pong(_) => {}
            _ => {
                warn!("Unhandled WebSocket message type from user {}", user_id);
                break;
            }
        }
    }

    state.users.remove(&user_id);
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MessageData {
    images: Option<Vec<String>>,
    videos: Option<Vec<String>>,
    files: Option<Vec<String>>,
    text: Option<String>,
}

async fn handle_text(user_id: id, state: &State, text: String) -> Result<()> {
    if let Ok(mut message) = serde_json::from_str::<Message<MessageData>>(&text) {
        message.from = user_id;

        let now = Utc::now();
        if (now - message.time).abs() > Duration::milliseconds(500) {
            if let Some(mut session) = state.users.get_mut(&user_id) {
                let message = Message {
                    data: json!({
                        "op": "change_time",
                        "time": now,
                        "id": message.id
                    }),
                    time: Utc::now(),
                    r#type: MessageType::Server,
                    ..Default::default()
                };

                session.ws_conn.text(json!(message).to_string());
            }
            message.time = Utc::now();
        }
        message.id = message.compute_id();

        let authorized = is_authorized(state, &message).await?;

        if !authorized {
            anyhow::bail!("Unauthorized to send message {:?}", message);
        }

        if matches!(message.r#type, MessageType::Direct | MessageType::Group) {
            db::save_message(&state.pool, &message)
                .await
                .context("Failed to save message")?;
        }

        dispatch_message(state, message).await;

        return Ok(());
    }

    if let Ok(mut event) = serde_json::from_str::<Message<Event>>(&text) {
        event.time = Utc::now();
        event.from = user_id;

        handle_event(state, event).await;
        return Ok(());
    }

    anyhow::bail!("Unkown message struct: {}", text)
}

async fn is_authorized<T>(state: &State, message: &Message<T>) -> Result<bool> {
    match message.r#type {
        MessageType::Direct | MessageType::Info => {
            Ok(db::are_friends(&state.pool, message.from, message.to).await?)
        }
        MessageType::Group | MessageType::InfoGroup => {
            Ok(db::in_group(&state.pool, message.from, message.to).await?)
        }
        _ => Ok(false),
    }
}

async fn dispatch_message<T>(state: &State, message: Message<T>)
where
    T: Serialize + Send + Sync + 'static,
{
    match message.r#type {
        MessageType::Direct | MessageType::Info => {
            send_direct_message(state, message).await;
        }
        MessageType::Group | MessageType::InfoGroup => {
            send_group_message(state, message).await;
        }
        _ => {}
    }
}

async fn send_direct_message<T: Serialize>(state: &State, message: Message<T>) {
    if let Some(mut user_to) = state.users.get_mut(&message.to) {
        user_to.ws_conn.text(json!(message).to_string()).await;
    }
}

async fn send_group_message<T>(state: &State, message: Message<T>)
where
    T: Serialize + Send + Sync + 'static,
{
    let users = db::get_group_users(&state.pool, message.to)
        .await
        .unwrap_or_default();

    let message = Arc::new(message);

    let mut conns = Vec::new();

    for user_id in users {
        if let Some(user_to) = state.users.get_mut(&user_id) {
            conns.push(user_to.ws_conn.clone());
        }
    }

    tokio::spawn(futures::stream::iter(conns).for_each_concurrent(Some(10), {
        move |mut conn| {
            let message = message.clone();
            async move {
                conn.text(json!(message).to_string()).await;
            }
        }
    }));
}

async fn send_info_message<T: Serialize>(state: &State, message: Message<T>) {
    if let Some(mut user_to) = state.users.get_mut(&message.to) {
        user_to.ws_conn.text(json!(message).to_string()).await;
    }
}

async fn handle_event(state: &State, event: Message<Event>) {
    match event.data {
        ChangeState(user_state) => {
            if let Some(mut user) = state.users.get_mut(&event.from) {
                let user_id = event.from;
                user.state = user_state;

                let friend_ids: Vec<id> = db::get_friends(&state.pool, user_id)
                    .await
                    .unwrap()
                    .iter()
                    .map(|f| f.id)
                    .collect();

                let mut message = Message::default();

                message.from = event.from;
                message.r#type = MessageType::Server;
                message.data = json!({
                    "event": "change_state",
                    "user": user_id,
                    "state": user.state
                });

                send_message_all(state, message, friend_ids).await;
            }
        }
        ExitChannel => {
            if let Some(mut user) = state.users.get_mut(&event.from) {
                let user_id = event.from;
                let Some(chat_id) = user.voice_chat else {
                    return;
                };

                user.voice_chat = None;

                let user_ids: Vec<Uuid> = if let Some(users) = state.voice_chats.get_mut(&chat_id) {
                    users.remove(&user_id);
                    users.iter().map(|r| *r.key()).collect()
                } else {
                    return;
                };

                let message = Message {
                    from: user_id,
                    r#type: MessageType::Server,
                    data: json!({
                        "event": "exit_channel",
                        "user": user_id,
                        "channel": chat_id
                    }),
                    ..Default::default()
                };

                send_message_all(state, message, user_ids).await;
            }
        }
        JoinChannel { id } => {
            if let Some(mut user) = state.users.get_mut(&event.from) {
                let user_id = event.from;
                let chat_id = id;

                user.voice_chat = Some(chat_id);

                let users = state
                    .voice_chats
                    .entry(chat_id)
                    .or_insert_with(DashSet::new);

                let user_ids = users.iter().map(|r| *r).collect();

                let message = Message {
                    from: user_id,
                    r#type: MessageType::Server,
                    data: json!({
                        "event": "join_channel",
                        "user": user_id,
                        "channel": chat_id
                    }),
                    ..Default::default()
                };

                send_message_all(state, message, user_ids).await;
            }
        }

        _ => {}
    }
}

async fn send_message_all<T: Serialize>(state: &State, message: Message<T>, user_ids: Vec<id>)
where
    T: Serialize + Send + Sync + 'static,
{
    let message = Arc::new(message);

    let conns: Vec<Session> = user_ids
        .into_iter()
        .filter_map(|user_id| state.users.get(&user_id))
        .map(|user_to| user_to.ws_conn.clone())
        .collect();

    tokio::spawn(futures::stream::iter(conns).for_each_concurrent(Some(10), {
        move |mut conn| {
            let message = message.clone();
            async move {
                conn.text(json!(message).to_string()).await;
            }
        }
    }));
}
