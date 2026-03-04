use crate::id::id;
use crate::msgpack::MsgPack;
use crate::state;
use crate::state::user::Status;
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

#[derive(Deserialize)]
struct UserQuery {
    id: Option<id>,
    username: Option<String>,
}

#[derive(Serialize)]
struct UserInfo {
    id: id,
    version: id,
    username: String,
    name: String,
    avatar: Option<String>,
    status: Status,
    friends: Vec<id>,
    groups: Vec<id>,
}

impl From<state::user::State> for UserInfo {
    fn from(value: state::user::State) -> Self {
        Self {
            id: value.id,
            version: value.version,
            username: value.username,
            name: value.name,
            avatar: value.avatar,
            status: value.status,
            friends: value.friends.into_iter().collect(),
            groups: value.groups,
        }
    }
}

async fn get_user(state: State, query: web::Query<UserQuery>) -> Result<MsgPack<UserInfo>, Error> {
    if let Some(uid) = query.id {
        if let Some(user) = state.users.get(&uid) {
            return Ok(MsgPack(user.state.clone().into()));
        }

        let user = db::user::get_user(&state.pool, uid)
            .await
            .map_err(|e| {
                warn!("Error while getting user: {}", e);
                error::ErrorInternalServerError("Error while getting user")
            })?
            .ok_or(error::ErrorNotFound("User not found"))?;

        let friends = db::user::get_friends(&state.pool, uid).await.map_err(|e| {
            warn!("Error while getting user friends: {}", e);
            error::ErrorInternalServerError("Error while getting user friends")
        })?;

        let groups = db::user::get_groups(&state.pool, uid).await.map_err(|e| {
            warn!("Error while getting user groups: {}", e);
            error::ErrorInternalServerError("Error while getting user groups")
        })?;

        return Ok(MsgPack(UserInfo {
            id: user.id,
            version: id(0),
            username: user.username,
            name: user.name,
            avatar: user.avatar,
            status: Status::Offline,
            friends,
            groups,
        }));
    }

    if let Some(username) = &query.username {
        let user = db::user::get_user_by_username(&state.pool, username)
            .await
            .map_err(|e| {
                warn!("Error while getting user: {}", e);
                error::ErrorInternalServerError("Error while getting user")
            })?
            .ok_or(error::ErrorNotFound("User not found"))?;

        let friends = db::user::get_friends(&state.pool, user.id)
            .await
            .map_err(|e| {
                warn!("Error while getting user friends: {}", e);
                error::ErrorInternalServerError("Error while getting user friends")
            })?;

        let groups = db::user::get_groups(&state.pool, user.id)
            .await
            .map_err(|e| {
                warn!("Error while getting user groups: {}", e);
                error::ErrorInternalServerError("Error while getting user groups")
            })?;

        return Ok(MsgPack(UserInfo {
            id: user.id,
            version: id(0),
            username: user.username,
            name: user.name,
            avatar: user.avatar,
            status: Status::Offline,
            friends,
            groups,
        }));
    }

    Err(error::ErrorBadRequest("No query parameters provided"))
}

#[derive(Serialize)]
struct UsersInfo {
    id: id,
    version: id,
    username: String,
    name: String,
    avatar: Option<String>,
    status: Status,
}

impl From<user::State> for UsersInfo {
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

impl From<db::user::User> for UsersInfo {
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
    version: id,
    name: String,
    icon: Option<String>,
}

impl From<&state::group::Group> for GroupInfo {
    fn from(value: &state::group::Group) -> Self {
        Self {
            id: value.id,
            version: value.version,
            name: value.name.clone(),
            icon: value.icon.clone(),
        }
    }
}

impl From<db::group::Group> for GroupInfo {
    fn from(value: db::group::Group) -> Self {
        Self {
            id: value.id,
            version: id(0),
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

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/info")
            .route("/search_user", web::get().to(search_user))
            .route("/user", web::get().to(get_user))
            .route("/users", web::post().to(get_users))
            .route("/groups", web::post().to(get_groups)),
    );
}
