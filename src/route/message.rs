use crate::State;
use crate::auth::JwtUser;
use crate::db;
use crate::models::{Message, MessageType, State as UserState, User};
use actix_web::web;
use actix_web::{Error, HttpRequest, HttpResponse};
use actix_ws::Message as WsMessage;
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
                        match message.r#type {
                            MessageType::Direct | MessageType::Info => {
                                if !db::are_friends(&state.pool, user_id, message.to)
                                    .await
                                    .unwrap()
                                {
                                    continue;
                                }
                            }
                            MessageType::Group => {
                                if !db::in_group(&state.pool, user_id, message.to)
                                    .await
                                    .unwrap()
                                {
                                    continue;
                                }
                            }
                            _ => {}
                        }

                        if matches!(message.r#type, MessageType::Direct | MessageType::Group) {
                            if (message.time - Utc::now()).num_seconds().abs() > 10 {
                                message.time = Utc::now();
                            }

                            db::save_message(&state.pool, user_id, &message)
                                .await
                                .unwrap();
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
                                    Some(50),
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
                }

                WsMessage::Close(_) => {
                    state.users.remove(&user_id);

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
