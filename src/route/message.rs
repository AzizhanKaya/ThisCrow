use crate::State;
use crate::auth::JwtUser;
use crate::db;
use crate::models::Events::{ChangeState, ExitChannel, FriendReq, JoinChannel, JoinReq};
use crate::models::{Event, Message, MessageType, State as UserState, User, id};
use actix_web::{Error, HttpRequest, HttpResponse};
use actix_web::{error, web};
use actix_ws::Message as WsMessage;
use actix_ws::MessageStream;
use chrono::Utc;
use futures_util::StreamExt;
use log::warn;
use serde_json::json;
use std::sync::Arc;

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
                    warn!("Failed to handle text message from {}:\n {:#?}", user_id, e);
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

async fn handle_text(user_id: id, state: &State, text: String) -> Result<(), Error> {
    if let Ok(mut message) = serde_json::from_str::<Message>(&text) {
        message.time = Utc::now();
        message.from = user_id;

        let authorized = is_authorized(state, &message).await;

        if !authorized {
            return Err(error::ErrorUnauthorized(
                "Not authorized to send this message",
            ));
        }

        if matches!(message.r#type, MessageType::Direct | MessageType::Group) {
            db::save_message(&state.pool, &message).await.map_err(|e| {
                error::ErrorInternalServerError(format!("Failed to save message: {}", e))
            })?;
        }

        dispatch_message(state, message).await;

        return Ok(());
    }

    if let Ok(event) = serde_json::from_str::<Event>(&text) {
        handle_event(state, user_id, event).await;
        return Ok(());
    }

    Err(error::ErrorBadRequest(text))
}

async fn is_authorized(state: &State, message: &Message) -> bool {
    match message.r#type {
        MessageType::Direct | MessageType::Info => {
            db::are_friends(&state.pool, message.from, message.to)
                .await
                .unwrap_or(false)
        }
        MessageType::Group => db::in_group(&state.pool, message.from, message.to)
            .await
            .unwrap_or(false),
        _ => false,
    }
}

async fn dispatch_message(state: &State, message: Message) {
    match message.r#type {
        MessageType::Direct => {
            send_direct_message(state, message).await;
        }
        MessageType::Group => {
            send_group_message(state, message).await;
        }
        MessageType::Info => {
            send_info_message(state, message).await;
        }
        _ => {}
    }
}

async fn send_direct_message(state: &State, message: Message) {
    if let Some(mut user_to) = state.users.get_mut(&message.to) {
        user_to.ws_conn.text(json!(message).to_string()).await;
    }
}

async fn send_group_message(state: &State, message: Message) {
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

async fn send_info_message(state: &State, message: Message) {
    if let Some(mut user_to) = state.users.get_mut(&message.to) {
        user_to.ws_conn.text(json!(message).to_string()).await;
    }
}

async fn handle_event(state: &State, user_id: id, event: Event) {
    match event.types {
        ChangeState(user_state) => {
            if let Some(mut user) = state.users.get_mut(&user_id) {
                user.state = user_state.clone();

                let friend_ids: Vec<id> = db::get_friends(&state.pool, user_id)
                    .await
                    .unwrap()
                    .iter()
                    .map(|f| f.id)
                    .collect();

                let mut message = Message::default();

                message.from = user_id;
                message.r#type = MessageType::Server;
                message.data = json!({
                    "event": "changed_state",
                    "state": user_state
                });

                send_message_all(state, message, friend_ids).await;
            }
        }
        ExitChannel { direct: false } => {
            if let Some((_, chat_id)) = state.chat_users.remove(&user_id) {
                let user_ids: Vec<id> = state
                    .chat_users
                    .iter()
                    .filter(|kv| *kv.value() == chat_id)
                    .map(|kv| *kv.key())
                    .collect();

                let mut message = Message::default();

                message.from = user_id;
                message.r#type = MessageType::Server;
                message.data = json!({
                    "event": "exit_channel",
                });

                send_message_all(state, message, user_ids).await;
            }
        }
        ExitChannel { direct: true } => {
            if let Some((_, chat_id)) = state.chat_users.remove(&user_id) {
                let user_ids: Vec<id> = state
                    .chat_users
                    .iter()
                    .filter(|kv| *kv.value() == chat_id)
                    .map(|kv| *kv.key())
                    .collect();

                let mut message = Message::default();

                message.from = user_id;
                message.r#type = MessageType::Server;
                message.data = json!({
                    "event": "exit_channel",
                });

                send_message_all(state, message, user_ids).await;
            }
        }
        JoinChannel { id, direct: true } => {
            if let Some(user) = state.chat_users.get_mut(&user_id) {
                todo!()
            }
        }
        JoinChannel { id, direct: false } => {
            if let Some(user) = state.chat_users.get_mut(&user_id) {
                todo!()
            }
        }
        _ => {}
    }
}

async fn send_message_all(state: &State, message: Message, user_ids: Vec<id>) {
    let message = Arc::new(message);

    let mut conns = Vec::new();

    for user_id in user_ids {
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
