use crate::id::id;
use crate::state::group::Permissions;
use crate::{State, middleware::JwtUser};
use actix_web::{Error, HttpResponse, error, web};
use chrono::{DateTime, Utc};
use log::warn;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct Me {
    id: id,
    version: id,
    username: String,
    avatar: Option<String>,
}

pub async fn me(state: State, user: web::ReqData<JwtUser>) -> Result<HttpResponse, Error> {
    let user = state
        .users
        .get(&user.id)
        .ok_or_else(|| error::ErrorBadRequest("Session has not initialized"))?;

    let me = Me {
        id: user.id,
        version: user.get_version(),
        username: user.state.username.clone(),
        avatar: user.state.avatar.clone(),
    };

    Ok(HttpResponse::Ok().json(me))
}

#[derive(Deserialize, Debug)]
pub struct MessagesQuery {
    user_id: id,
    len: Option<i64>,
    start: Option<DateTime<Utc>>,
    end: DateTime<Utc>,
}

pub async fn get_messages(
    state: State,
    user: web::ReqData<JwtUser>,
    query: web::Query<MessagesQuery>,
) -> Result<HttpResponse, Error> {
    let messages = state
        .messages
        .get_direct_messages(user.id, query.user_id, query.start, query.end, query.len)
        .map_err(|e| {
            warn!("Error while getting messages: {}", e);
            error::ErrorInternalServerError("Error while getting messages")
        })?;

    Ok(HttpResponse::Ok().json(messages))
}

#[derive(Deserialize, Debug)]
pub struct ChannelMessagesQuery {
    group_id: id,
    channel_id: id,
    len: Option<i64>,
    start: Option<DateTime<Utc>>,
    end: DateTime<Utc>,
}

pub async fn get_channel_messages(
    state: State,
    user: web::ReqData<JwtUser>,
    query: web::Query<ChannelMessagesQuery>,
) -> Result<HttpResponse, Error> {
    state
        .groups
        .get(&query.group_id)
        .filter(|group| {
            group
                .compute_permissions(user.id, Some(query.channel_id))
                .contains(Permissions::VIEW_MESSAGES)
        })
        .ok_or_else(|| error::ErrorUnauthorized(""))?;

    let messages = state
        .messages
        .get_channel_messages(
            query.group_id,
            query.channel_id,
            query.start,
            query.end,
            query.len,
        )
        .map_err(|e| {
            warn!("Error while getting messages: {}", e);
            error::ErrorInternalServerError("Error while getting messages")
        })?;

    Ok(HttpResponse::Ok().json(messages))
}

pub async fn get_friends(state: State, user: web::ReqData<JwtUser>) -> Result<HttpResponse, Error> {
    let friends = state
        .users
        .get(&user.id)
        .ok_or_else(|| error::ErrorBadRequest("Session has not initialized"))?
        .state
        .friends
        .clone();

    Ok(HttpResponse::Ok().json(friends))
}

#[derive(Serialize)]
pub struct FriendRequets {
    incoming: Vec<id>,
    outgoing: Vec<id>,
    version: id,
}

pub async fn get_friend_requets(
    state: State,
    user: web::ReqData<JwtUser>,
) -> Result<HttpResponse, Error> {
    if let Some(user) = state.users.get(&user.id) {
        let incoming = user.state.friend_requests.clone();
        let outgoing = user.state.friend_requests_sent.clone();
        let version = user.get_version();

        return Ok(HttpResponse::Ok().json(FriendRequets {
            incoming,
            outgoing,
            version,
        }));
    }

    Err(error::ErrorUnauthorized("Session has not initialized"))
}

pub async fn get_groups(state: State, user: web::ReqData<JwtUser>) -> Result<HttpResponse, Error> {
    let groups = state
        .users
        .get(&user.id)
        .ok_or_else(|| error::ErrorBadRequest("Session has not initialized"))?
        .state
        .groups
        .clone();

    Ok(HttpResponse::Ok().json(groups))
}

pub async fn get_dms(state: State, user: web::ReqData<JwtUser>) -> Result<HttpResponse, Error> {
    let dms = state
        .users
        .get(&user.id)
        .ok_or_else(|| error::ErrorBadRequest("Session has not initialized"))?
        .state
        .dms
        .clone();

    Ok(HttpResponse::Ok().json(dms))
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/state")
            .route("/messages", web::get().to(get_messages))
            .route("/friends", web::get().to(get_friends))
            .route("/friend_requests", web::get().to(get_friend_requets))
            .route("/dms", web::get().to(get_dms))
            .route("/groups", web::get().to(get_groups))
            .route("/me", web::get().to(me)),
    );
}
