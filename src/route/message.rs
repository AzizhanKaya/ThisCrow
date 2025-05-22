use crate::State;
use crate::auth::JwtUser;
use crate::models::{Event, Message, MessageType, State as UserState, User};
use crate::route::webrtc::process_answer;
use crate::route::webrtc::process_offer;
use actix_web::web;
use actix_web::{Error, HttpRequest, HttpResponse};
use actix_ws::Message as WsMessage;
use chrono::Utc;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
enum WebRTC {
    answer(Value),
    offer(Value),
}

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
                            MessageType::Direct => {
                                let mut user_to = state.users.get_mut(&user_id).unwrap();

                                message.from = user_id;
                                message.time = Utc::now();

                                user_to.ws_conn.text(json!(message).to_string());
                            }

                            MessageType::Group => {
                                let mut user_to = state.users.get_mut(&user_id).unwrap();

                                message.from = user_id;
                                message.time = Utc::now();

                                user_to.ws_conn.text(json!(message).to_string());
                            }
                            _ => {}
                        }
                    }

                    if let Ok(event) = serde_json::from_str::<Event>(&text) {
                        match event.r#type {
                            _ => {}
                        }
                    }

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
                }

                WsMessage::Close(_) => {
                    state.users.remove(&user_id);
                    break;
                }

                WsMessage::Ping(ping) => {
                    let _ = session.pong(&ping).await;
                }

                _ => {}
            }
        }
    });

    Ok(response)
}

macro_rules! handle_enum_dispatch {
    ($enum_type:ident, $value:expr, $($arg:expr),*) => {
        match $value {
            $( $enum_type::$variant(data) => {
                paste! {
                    [<$enum_type _ $variant _handle>](data, $($arg),*).await
                }
            }, )*
        }
    };
}
