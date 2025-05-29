use crate::State;
use crate::auth::JwtUser;
use crate::db;
use crate::models::{Event, Message, MessageType, State as UserState, User, id};
use crate::route::webrtc::process_answer;
use crate::route::webrtc::process_offer;
use actix_web::web;
use actix_web::{Error, HttpRequest, HttpResponse};
use actix_ws::Message as WsMessage;
use chrono::Utc;
use futures_util::StreamExt;
use log::warn;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;
use webrtc::data::message;

pub async fn ws(
    req: HttpRequest,
    stream: web::Payload,
    state: State,
    user: web::ReqData<JwtUser>,
) -> Result<HttpResponse, Error> {
    let (response, mut session, mut msg_stream) = actix_ws::handle(&req, stream)?;

    let user_id = user.id;
    let username = user.username.clone();

    let user = User {
        username: username.clone(),
        state: UserState::Online,
        ws_conn: session.clone(),
    };

    state.users.insert(user_id, user);

    actix_web::rt::spawn(async move {
        while let Some(Ok(msg)) = msg_stream.next().await {
            match msg {
                WsMessage::Text(text) => {
                    if let Ok(mut message) = serde_json::from_str::<Message>(&text) {
                        if matches!(
                            message.r#type,
                            MessageType::Direct | MessageType::Server | MessageType::Group
                        ) {
                            db::save_message(
                                &state.pool,
                                user_id,
                                message.to,
                                message.data.clone(),
                                message.r#type.clone(),
                            );
                        }
                        match message.r#type {
                            MessageType::Direct => {
                                let Some(mut user_to) = state.users.get_mut(&message.to) else {
                                    continue;
                                };

                                message.from = user_id;
                                message.time = Utc::now();

                                user_to.ws_conn.text(json!(message).to_string()).await;
                            }

                            MessageType::Group => {
                                let users =
                                    db::get_group_users(&state.pool, message.to).await.unwrap();

                                message.from = user_id;
                                message.time = Utc::now();

                                let state = state.clone();

                                let message = Arc::new(message);

                                tokio::spawn(futures::stream::iter(users).for_each_concurrent(
                                    None,
                                    move |user_id| {
                                        let state = state.clone();
                                        let message = message.clone();
                                        async move {
                                            if let Some(mut user_to) = state.users.get_mut(&user_id)
                                            {
                                                user_to
                                                    .ws_conn
                                                    .text(json!(message).to_string())
                                                    .await;
                                            }
                                        }
                                    },
                                ));
                            }

                            MessageType::Info => {
                                let Some(mut user_to) = state.users.get_mut(&message.to) else {
                                    continue;
                                };

                                message.from = user_id;
                                message.time = Utc::now();

                                user_to
                                    .ws_conn
                                    .text(json!(message).to_string())
                                    .await
                                    .is_err();
                            }

                            _ => {}
                        }
                    }

                    /*
                    if let Ok(web_rtc) = serde_json::from_str::<WebRTC>(&text) {
                        match web_rtc {
                            WebRTC::answer(answer) => {
                                let chat_id = answer.get("chat_id").unwrap().as_str().unwrap();
                                let chat_id = Uuid::try_parse(chat_id).unwrap();

                                let result =
                                    match process_answer(user_id, chat_id, answer, state.clone())
                                        .await
                                    {
                                        Ok(answer) => answer.to_string(),
                                        Err(e) => json!({
                                                "type": "error",
                                                "error": e.to_string()
                                        })
                                        .to_string(),
                                    };

                                session.text(result).await;
                            }
                            WebRTC::offer(offer) => {
                                let chat_id = offer.get("chat_id").unwrap().as_str().unwrap();
                                let chat_id = Uuid::try_parse(chat_id).unwrap();

                                let answer =
                                    match process_offer(user_id, chat_id, offer, state.clone())
                                        .await
                                    {
                                        Ok(answer) => answer.to_string(),
                                        Err(e) => json!({
                                                "type": "error",
                                                "error": e.to_string()
                                        })
                                        .to_string(),
                                    };

                                session.text(answer).await;
                            }

                        }

                    }
                    */
                }

                WsMessage::Close(_) => {
                    let cleanup_state = state.clone();

                    tokio::spawn(async move {
                        cleanup_user_disconnect(cleanup_state, user_id).await;
                    });

                    break;
                }

                WsMessage::Ping(ping) => {
                    let _ = session.pong(&ping).await;
                }

                WsMessage::Pong(_) => {}

                _ => {
                    warn!("Unhandled WebSocket message type from user {}", user_id);
                }
            }
        }
    });

    Ok(response)
}

async fn cleanup_user_disconnect(state: State, user_id: id) {
    state.users.remove(&user_id);

    let mut chats_to_remove = Vec::new();

    for chat_entry in state.chats.iter() {
        let chat_id = *chat_entry.key();
        let chat = chat_entry.value();

        if chat.users.contains_key(&user_id) {
            chat.users.remove(&user_id);

            if chat.users.is_empty() {
                chats_to_remove.push(chat_id);
            }
        }
    }

    for chat_id in chats_to_remove {
        state.chats.remove(&chat_id);
    }
}
