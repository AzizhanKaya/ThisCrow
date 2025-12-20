use crate::db::AddFriend;
use crate::models::{Message, MessageType};
use crate::{State, auth::JwtUser, db, models::id};
use actix_web::{Error, HttpResponse, error, web};
use chrono::Utc;
use log::warn;
use serde::Deserialize;
use serde_json::json;
#[derive(Deserialize)]
pub struct Username {
    username: String,
}

pub async fn search_users(
    state: State,
    query: web::Query<Username>,
) -> Result<HttpResponse, Error> {
    let users = db::get_users_like(&state.pool, &query.username)
        .await
        .map_err(|e| {
            warn!("Error while searching users: {}", e);
            error::ErrorInternalServerError("Error while searching users")
        })?;

    Ok(HttpResponse::Ok().json(users))
}

#[derive(Deserialize)]
pub struct UserId {
    user_id: id,
}

pub async fn add_friend(
    state: State,
    user: web::ReqData<JwtUser>,
    to: web::Json<UserId>,
) -> Result<HttpResponse, Error> {
    let res = db::add_friend(&state.pool, user.id, to.user_id)
        .await
        .map_err(|e| {
            warn!("Error while adding friends: {}", e);
            error::ErrorInternalServerError("Error while adding friend")
        })?;

    if let Some(mut session) = state.users.get_mut(&to.user_id) {
        let message_type = match res {
            AddFriend::Add => "friend_added",
            AddFriend::Request => "friend_request",
        };

        let message = Message {
            from: user.id,
            data: json!({
                "type": message_type
            }),
            time: Utc::now(),
            r#type: MessageType::Info,
            ..Default::default()
        };

        session.ws.text(json!(message).to_string());
    }

    Ok(HttpResponse::Ok().json(json!({
        "action": match res {
            AddFriend::Add => "friend_added",
            AddFriend::Request => "friend_request_sent"
        }
    })))
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/event")
            .route("/search_users", web::get().to(search_users))
            .route("/add_friend", web::post().to(add_friend)),
    );
}
