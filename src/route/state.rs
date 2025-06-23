use crate::models::State as StateUser;
use crate::{State, auth::JwtUser, db, models::id};
use actix_web::{Error, HttpResponse, error, web};
use chrono::{DateTime, Utc};
use db::UserDB;
use log::warn;
use serde::{Deserialize, Serialize};

pub async fn me(state: State, user: web::ReqData<JwtUser>) -> Result<HttpResponse, Error> {
    let user = db::get_user(&state.pool, user.id)
        .await
        .ok_or(error::ErrorUnauthorized(""))?;

    Ok(HttpResponse::Ok().json(user))
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

    Ok(HttpResponse::Ok().json(messages))
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
    Ok(HttpResponse::Ok().json(response_users))
}

pub async fn get_groups(state: State, user: web::ReqData<JwtUser>) -> Result<HttpResponse, Error> {
    let grops = db::get_groups(&state.pool, user.id).await.map_err(|e| {
        warn!("Error while getting groups: {}", e);
        error::ErrorInternalServerError("Error while getting groups")
    })?;
    Ok(HttpResponse::Ok().json(grops))
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/state")
            .route("/messages", web::get().to(get_messages))
            .route("/friends", web::get().to(get_friends))
            .route("/groups", web::get().to(get_groups))
            .route("/me", web::get().to(me)),
    );
}
