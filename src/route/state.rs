use crate::id::id;
use crate::message::Message;
use crate::msgpack::MsgPack;
use crate::state::group::Permissions;
use crate::{State, middleware::JwtUser};
use actix_web::{Error, error, web};
use chrono::{DateTime, Utc};
use log::warn;
use rmpv::Value;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub async fn me(user: web::ReqData<JwtUser>) -> Result<MsgPack<JwtUser>, Error> {
    Ok(MsgPack(user.into_inner()))
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
) -> Result<MsgPack<Vec<Message<Value>>>, Error> {
    let messages = state
        .messages
        .get_direct_messages(user.id, query.user_id, query.start, query.end, query.len)
        .map_err(|e| {
            warn!("Error while getting messages: {}", e);
            error::ErrorInternalServerError("Error while getting messages")
        })?;

    Ok(MsgPack(messages))
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
) -> Result<MsgPack<Vec<Message<Value>>>, Error> {
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

    Ok(MsgPack(messages))
}

pub async fn get_friends(
    state: State,
    user: web::ReqData<JwtUser>,
) -> Result<MsgPack<HashSet<id>>, Error> {
    let friends = state
        .users
        .get(&user.id)
        .ok_or_else(|| error::ErrorBadRequest("Session has not initialized"))?
        .state
        .friends
        .clone();

    Ok(MsgPack(friends))
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
) -> Result<MsgPack<FriendRequets>, Error> {
    if let Some(user) = state.users.get(&user.id) {
        let incoming = user.state.friend_requests.clone();
        let outgoing = user.state.friend_requests_sent.clone();
        let version = user.get_version();

        return Ok(MsgPack(FriendRequets {
            incoming,
            outgoing,
            version,
        }));
    }

    Err(error::ErrorUnauthorized("Session has not initialized"))
}

pub async fn get_groups(
    state: State,
    user: web::ReqData<JwtUser>,
) -> Result<MsgPack<Vec<id>>, Error> {
    let groups = state
        .users
        .get(&user.id)
        .ok_or_else(|| error::ErrorBadRequest("Session has not initialized"))?
        .state
        .groups
        .clone();

    Ok(MsgPack(groups))
}

pub async fn get_dms(state: State, user: web::ReqData<JwtUser>) -> Result<MsgPack<Vec<id>>, Error> {
    let dms = state
        .users
        .get(&user.id)
        .ok_or_else(|| error::ErrorBadRequest("Session has not initialized"))?
        .state
        .dms
        .clone();

    Ok(MsgPack(dms))
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
