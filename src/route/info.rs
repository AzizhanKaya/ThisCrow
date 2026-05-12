use crate::id::id;
use crate::msgpack::MsgPack;
use crate::state;
use crate::state::user::{Activity, Status};
use crate::{State, db, state::user};
use actix_web::error::ErrorInternalServerError;
use actix_web::{Error, error, web};
use itertools::Itertools;
use log::warn;
use serde::{Deserialize, Serialize};
use std::ops::Deref;

#[derive(Deserialize)]
struct Username {
    username: String,
}

async fn search_user(
    state: State,
    query: web::Query<Username>,
) -> Result<MsgPack<Vec<UsersInfo>>, Error> {
    let users = db::user::get_users_like(&state.pool, &query.username)
        .await
        .map_err(|e| {
            warn!("Error while searching users: {}", e);
            error::ErrorInternalServerError("Error while searching users")
        })?;

    Ok(MsgPack(users.into_iter().map(|u| u.into()).collect()))
}

#[derive(Serialize)]
struct UserInfo {
    id: id,
    username: String,
    name: String,
    avatar: Option<String>,
    banner: Option<String>,
    status: Status,
    friends: Vec<id>,
    groups: Vec<id>,
    activities: Vec<Activity>,
}

impl From<state::user::State> for UserInfo {
    fn from(value: state::user::State) -> Self {
        Self {
            id: value.id,
            username: value.username,
            name: value.name,
            avatar: value.avatar,
            banner: value.banner,
            status: value.status,
            friends: value.friends.into_iter().collect(),
            groups: value.groups,
            activities: value.activities,
        }
    }
}

async fn get_user(state: State, user_id: web::Path<id>) -> Result<MsgPack<UserInfo>, Error> {
    let user_id = user_id.into_inner();

    if let Some(user) = state.users.get(&user_id) {
        return Ok(MsgPack(user.state.clone().into()));
    }

    let user = db::user::get_user(&state.pool, user_id)
        .await
        .map_err(|e| {
            warn!("Error while getting user: {}", e);
            error::ErrorInternalServerError("Error while getting user")
        })?;

    let friends = db::user::get_friends(&state.pool, user_id)
        .await
        .map_err(|e| {
            warn!("Error while getting user friends: {}", e);
            error::ErrorInternalServerError("Error while getting user friends")
        })?;

    let groups = db::user::get_groups(&state.pool, user_id)
        .await
        .map_err(|e| {
            warn!("Error while getting user groups: {}", e);
            error::ErrorInternalServerError("Error while getting user groups")
        })?;

    Ok(MsgPack(UserInfo {
        id: user.id,
        username: user.username,
        name: user.name,
        avatar: user.avatar,
        banner: user.banner,
        status: Status::Offline,
        friends,
        groups,
        activities: Vec::new(),
    }))
}

#[derive(Serialize)]
struct UsersInfo {
    id: id,
    username: String,
    name: String,
    avatar: Option<String>,
    banner: Option<String>,
    status: Status,
    activities: Vec<Activity>,
}

impl From<user::State> for UsersInfo {
    fn from(value: user::State) -> Self {
        Self {
            id: value.id,
            username: value.username,
            name: value.name,
            avatar: value.avatar,
            banner: value.banner,
            status: value.status,
            activities: value.activities,
        }
    }
}

impl From<db::user::User> for UsersInfo {
    fn from(value: db::user::User) -> Self {
        Self {
            id: value.id,
            username: value.username,
            name: value.name,
            avatar: value.avatar,
            banner: value.banner,
            status: Status::Offline,
            activities: Vec::new(),
        }
    }
}

async fn get_users(
    state: State,
    MsgPack(users): MsgPack<Vec<id>>,
) -> Result<MsgPack<Vec<UsersInfo>>, Error> {
    let (in_state, in_db): (Vec<UsersInfo>, Vec<id>) =
        users
            .into_iter()
            .partition_map(|uid| match state.users.get(&uid) {
                Some(u) => itertools::Either::Left(u.state.clone().into()),
                None => itertools::Either::Right(uid),
            });

    let in_db: Vec<UsersInfo> = db::user::get_users_by_ids(&state.pool, in_db)
        .await
        .map_err(ErrorInternalServerError)?
        .into_iter()
        .map(|u| u.into())
        .collect();

    Ok(MsgPack(
        in_state
            .into_iter()
            .chain(in_db)
            .collect::<Vec<UsersInfo>>(),
    ))
}

#[derive(Serialize)]
struct GroupInfo {
    id: id,
    name: String,
    icon: Option<String>,
}

impl From<&state::group::Group> for GroupInfo {
    fn from(value: &state::group::Group) -> Self {
        Self {
            id: value.id,
            name: value.name.clone(),
            icon: value.icon.clone(),
        }
    }
}

impl From<db::group::Group> for GroupInfo {
    fn from(value: db::group::Group) -> Self {
        Self {
            id: value.id,
            name: value.name,
            icon: value.icon,
        }
    }
}

async fn get_groups(
    state: State,
    MsgPack(groups): MsgPack<Vec<id>>,
) -> Result<MsgPack<Vec<GroupInfo>>, Error> {
    let (in_state, in_db): (Vec<GroupInfo>, Vec<id>) =
        groups
            .into_iter()
            .partition_map(|gid| match state.groups.get(&gid) {
                Some(g) => itertools::Either::Left(g.deref().into()),
                None => itertools::Either::Right(gid),
            });

    let in_db: Vec<GroupInfo> = db::group::get_groups_info(&state.pool, in_db)
        .await
        .map_err(ErrorInternalServerError)?
        .into_iter()
        .map(|g| g.into())
        .collect();

    Ok(MsgPack(
        in_state
            .into_iter()
            .chain(in_db)
            .collect::<Vec<GroupInfo>>(),
    ))
}

async fn get_public_key(state: State, user_id: web::Path<id>) -> Result<MsgPack<Vec<u8>>, Error> {
    let user_id = user_id.into_inner();

    let user = db::user::get_user(&state.pool, user_id)
        .await
        .map_err(|e| {
            warn!("Error while getting user: {}", e);
            error::ErrorInternalServerError("Error while getting user")
        })?;

    Ok(MsgPack(user.public_key))
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/info")
            .route("/search_user", web::get().to(search_user))
            .route("/user/{id}", web::get().to(get_user))
            .route("/public_key/{id}", web::get().to(get_public_key))
            .route("/users", web::post().to(get_users))
            .route("/groups", web::post().to(get_groups)),
    );
}
