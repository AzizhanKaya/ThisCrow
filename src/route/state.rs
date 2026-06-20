use crate::db;
use crate::id::id;
use crate::message::snowflake::snowflake_id;
use crate::msgpack::MsgPack;
use crate::{State, middleware::JwtUser};
use actix_web::{Error, error, web};
use log::warn;
use serde::Serialize;
use std::collections::HashSet;

pub async fn me(user: web::ReqData<JwtUser>) -> Result<MsgPack<JwtUser>, Error> {
    Ok(MsgPack(user.into_inner()))
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
}

pub async fn get_friend_requets(
    state: State,
    user: web::ReqData<JwtUser>,
) -> Result<MsgPack<FriendRequets>, Error> {
    if let Some(user) = state.users.get(&user.id) {
        let incoming = user.state.friend_requests.clone();
        let outgoing = user.state.friend_requests_sent.clone();

        return Ok(MsgPack(FriendRequets { incoming, outgoing }));
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

pub async fn get_dms(
    state: State,
    user: web::ReqData<JwtUser>,
) -> Result<MsgPack<Vec<(id, snowflake_id)>>, Error> {
    let dms = state.messages.get_dms(user.id).await.map_err(|e| {
        warn!("Error while getting dms: {}", e);
        error::ErrorInternalServerError("Error while getting dms")
    })?;

    Ok(MsgPack(dms))
}

pub async fn get_voice_direct(
    state: State,
    user: web::ReqData<JwtUser>,
) -> Result<MsgPack<HashSet<id>>, Error> {
    let voice_direct = state
        .voice_direct
        .get(&user.id)
        .map(|v| v.value().clone())
        .unwrap_or_default();

    Ok(MsgPack(voice_direct))
}

pub async fn get_blocks(
    state: State,
    user: web::ReqData<JwtUser>,
) -> Result<MsgPack<Vec<id>>, Error> {
    let blocks = db::user::get_blocks(&state.pool, user.id)
        .await
        .map_err(|e| {
            warn!("Error while getting blocks: {}", e);
            error::ErrorInternalServerError("Error while getting blocks")
        })?;

    Ok(MsgPack(blocks))
}

pub async fn get_blocked_by(
    state: State,
    user: web::ReqData<JwtUser>,
) -> Result<MsgPack<Vec<id>>, Error> {
    let blocked_by = db::user::get_blocked_by(&state.pool, user.id)
        .await
        .map_err(|e| {
            warn!("Error while getting blocked_by: {}", e);
            error::ErrorInternalServerError("Error while getting blocked_by")
        })?;

    Ok(MsgPack(blocked_by))
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/state")
            .route("/friends", web::get().to(get_friends))
            .route("/friend_requests", web::get().to(get_friend_requets))
            .route("/dms", web::get().to(get_dms))
            .route("/groups", web::get().to(get_groups))
            .route("/me", web::get().to(me))
            .route("/voice_direct", web::get().to(get_voice_direct))
            .route("/blocks", web::get().to(get_blocks))
            .route("/blocked_by", web::get().to(get_blocked_by)),
    );
}
