use crate::id::id;
use crate::msgpack::MsgPack;
use crate::state::user::Status;
use crate::{State, db, state::user};
use actix_web::error::ErrorInternalServerError;
use actix_web::{Error, HttpResponse, error, web};
use itertools::Itertools;
use log::warn;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Deserialize)]
pub struct Username {
    username: String,
}

pub async fn search_users(
    state: State,
    query: web::Query<Username>,
) -> Result<HttpResponse, Error> {
    let users = db::user::get_users_like(&state.pool, &query.username)
        .await
        .map_err(|e| {
            warn!("Error while searching users: {}", e);
            error::ErrorInternalServerError("Error while searching users")
        })?;

    Ok(HttpResponse::Ok().json(users))
}

#[derive(Serialize)]
pub struct UserResponse {
    pub version: id,
    pub username: String,
    pub name: String,
    pub avatar: Option<String>,
    pub status: Status,
    pub friends: HashSet<id>,
    pub groups: Vec<id>,
}

pub async fn get_user(state: State, query: web::Query<id>) -> Result<MsgPack<UserResponse>, Error> {
    todo!()
}

#[derive(Serialize)]
pub struct UsersResponse {
    pub id: id,
    pub version: id,
    pub username: String,
    pub name: String,
    pub avatar: Option<String>,
    pub status: Status,
}

impl From<user::State> for UsersResponse {
    fn from(value: user::State) -> Self {
        Self {
            id: value.id,
            version: value.version,
            username: value.username,
            name: value.name,
            avatar: value.avatar,
            status: value.status,
        }
    }
}

impl From<db::user::User> for UsersResponse {
    fn from(value: db::user::User) -> Self {
        Self {
            id: value.id,
            version: id(0),
            username: value.username,
            name: value.name,
            avatar: value.avatar,
            status: Status::Offline,
        }
    }
}

pub async fn get_users(
    state: State,
    MsgPack(users): MsgPack<Vec<id>>,
) -> Result<MsgPack<Vec<UsersResponse>>, Error> {
    let (in_state, in_db): (Vec<UsersResponse>, Vec<id>) =
        users
            .into_iter()
            .partition_map(|uid| match state.users.get(&uid) {
                Some(u) => itertools::Either::Left(u.state.clone().into()),
                None => itertools::Either::Right(uid),
            });

    let in_db: Vec<UsersResponse> = db::user::get_users_by_ids(&state.pool, in_db)
        .await
        .map_err(ErrorInternalServerError)?
        .into_iter()
        .map(|u| u.into())
        .collect();

    Ok(MsgPack(
        in_state
            .into_iter()
            .chain(in_db)
            .collect::<Vec<UsersResponse>>(),
    ))
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/event")
            .route("/search_users", web::get().to(search_users))
            .route("/user", web::get().to(get_user))
            .route("/users", web::get().to(get_users)),
    );
}
