use crate::db;
use crate::id::id;
use crate::message::snowflake::snowflake_id;
use crate::msgpack::MsgPack;
use crate::{State, middleware::JwtUser};
use actix_web::cookie::Cookie;
use actix_web::cookie::time::Duration as CookieDuration;
use actix_web::{Error, HttpResponse, error, web};
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

pub async fn get_dms(
    state: State,
    user: web::ReqData<JwtUser>,
) -> Result<MsgPack<Vec<(id, snowflake_id)>>, Error> {
    let dms = state.messages.get_dms(user.id).map_err(|e| {
        warn!("Error while getting dms: {}", e);
        error::ErrorInternalServerError("Error while getting dms")
    })?;

    Ok(MsgPack(dms))
}

pub async fn log_out() -> Result<HttpResponse, Error> {
    Ok(HttpResponse::Ok()
        .cookie(
            Cookie::build("session", "")
                .path("/")
                .http_only(true)
                .max_age(CookieDuration::ZERO)
                .finish(),
        )
        .finish())
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/state")
            .route("/friends", web::get().to(get_friends))
            .route("/friend_requests", web::get().to(get_friend_requets))
            .route("/dms", web::get().to(get_dms))
            .route("/groups", web::get().to(get_groups))
            .route("/me", web::get().to(me))
            .route("/logout", web::get().to(log_out)),
    );
}
