use std::collections::HashSet;

use crate::db::AddFriend;
use crate::models::{Message, MessageType, State as StateUser};
use crate::{State, auth::JwtUser, db, models::id};
use actix_web::{Error, HttpResponse, error, web};
use chrono::{DateTime, Utc};
use db::UserDB;
use log::warn;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

pub async fn me(state: State, user: web::ReqData<JwtUser>) -> Result<HttpResponse, Error> {
    let user = db::get_user(&state.pool, user.id).await.unwrap();

    Ok(HttpResponse::Ok().json(json!(user)))
}

#[derive(Deserialize)]
pub struct MessagesQuery {
    user_id: id,
    len: Option<i64>,
    end: Option<DateTime<Utc>>,
}

pub async fn get_messages(
    state: State,
    user: web::ReqData<JwtUser>,
    query: web::Query<MessagesQuery>,
) -> Result<HttpResponse, Error> {
    let len = query.len.unwrap_or(50);
    let messages = db::get_messages(&state.pool, user.id, query.user_id, len, query.end)
        .await
        .unwrap();

    Ok(HttpResponse::Ok().json(json!(messages)))
}

#[derive(Serialize)]
pub struct User {
    #[serde(flatten)]
    user: UserDB,
    state: StateUser,
}

pub async fn get_friends(state: State, user: web::ReqData<JwtUser>) -> Result<HttpResponse, Error> {
    let friends = db::get_friends(&state.pool, user.id).await.map_err(|e| {
        warn!("Error while getting friends: {}", e);
        error::ErrorInternalServerError("Error while getting friends")
    })?;

    let response_users: Vec<User> = friends
        .into_iter()
        .map(|f| {
            let user_state = state
                .users
                .get(&f.id)
                .map(|u| u.state.clone())
                .unwrap_or(StateUser::Offline);

            User {
                user: f,
                state: user_state,
            }
        })
        .collect();
    Ok(HttpResponse::Ok().json(json!(response_users)))
}

pub async fn get_groups(state: State, user: web::ReqData<JwtUser>) -> Result<HttpResponse, Error> {
    let grops = db::get_groups(&state.pool, user.id).await.map_err(|e| {
        warn!("Error while getting groups: {}", e);
        error::ErrorInternalServerError("Error while getting groups:")
    })?;
    Ok(HttpResponse::Ok().json(json!(grops)))
}

#[derive(Deserialize)]
pub struct Username {
    username: String,
}

#[derive(Serialize)]
struct UserSearchResult {
    #[serde(flatten)]
    user: UserDB,
    is_friend: bool,
}

pub async fn search_users(
    state: State,
    user: web::ReqData<JwtUser>,
    query: web::Query<Username>,
) -> Result<HttpResponse, Error> {
    let users = db::get_users_like(&state.pool, &query.username)
        .await
        .map_err(|e| {
            warn!("Error while searching users: {}", e);
            error::ErrorInternalServerError("Error while searching users")
        })?;

    let friends = db::get_friends(&state.pool, user.id).await.map_err(|e| {
        warn!("Error while getting friends: {}", e);
        error::ErrorInternalServerError("Error while getting friends")
    })?;

    let friend_ids: HashSet<id> = friends.into_iter().map(|f| f.id).collect();

    let results: Vec<UserSearchResult> = users
        .into_iter()
        .map(|u| UserSearchResult {
            user: UserDB {
                id: u.id,
                avatar: u.avatar,
                name: u.name,
                username: u.username,
                ..Default::default()
            },
            is_friend: friend_ids.contains(&u.id),
        })
        .collect();

    Ok(HttpResponse::Ok().json(results))
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
            id: Uuid::new_v4(),
            from: user.id,
            data: json!({
                "type": message_type
            }),
            time: Utc::now(),
            r#type: MessageType::Info,
            ..Default::default()
        };

        session.ws_conn.text(json!(message).to_string());
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
        web::scope("/state")
            .route("/messages", web::get().to(get_messages))
            .route("/friends", web::get().to(get_friends))
            .route("/groups", web::get().to(get_groups))
            .route("/me", web::get().to(me))
            .route("/search_friends", web::get().to(search_users))
            .route("/add_friend", web::post().to(add_friend)),
    );
}
