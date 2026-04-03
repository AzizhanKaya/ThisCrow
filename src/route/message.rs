use crate::id::id;
use crate::message::{Data, snowflake::snowflake_id};
use crate::state::group::Permissions;
use crate::{State, db::message::StoredMessage, middleware::JwtUser, msgpack::MsgPack};
use actix_web::error;
use actix_web::{Error, error::ErrorInternalServerError, web};
use chrono::{DateTime, Utc};
use serde::Deserialize;

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
) -> Result<MsgPack<Vec<StoredMessage>>, Error> {
    let messages = state
        .messages
        .get_direct_messages(user.id, query.user_id, query.start, query.end, query.len)
        .map_err(|e| {
            log::error!("Error while getting messages: {:?}", e);
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
) -> Result<MsgPack<Vec<StoredMessage>>, Error> {
    state
        .groups
        .get(&query.group_id)
        .filter(|group| {
            group
                .compute_permissions(user.id, Some(query.channel_id))
                .contains(Permissions::VIEW_MESSAGES)
                || true
        })
        .ok_or_else(|| error::ErrorUnauthorized("Don't have permissions to view this channel"))?;

    let messages = state
        .messages
        .get_channel_messages(query.channel_id, query.start, query.end, query.len)
        .map_err(|e| {
            log::error!("Error while getting messages: {:?}", e);
            error::ErrorInternalServerError("Error while getting messages")
        })?;

    Ok(MsgPack(messages))
}

#[derive(Deserialize, Debug)]
pub struct Overwrite {
    message_id: snowflake_id,
    data: Data,
}

async fn overwrite_message(
    state: State,
    user: web::ReqData<JwtUser>,
    MsgPack(Overwrite { message_id, data }): MsgPack<Overwrite>,
) -> Result<(), Error> {
    let mut message = state
        .messages
        .get(message_id)
        .map_err(ErrorInternalServerError)?;

    message.overwrited = Some(());

    if message.from != user.id {
        return Err(error::ErrorUnauthorized(
            "You are not the author of this message",
        ));
    }

    if let Some(group_id) = message.group_id {
        let group = state
            .groups
            .get(&group_id)
            .ok_or_else(|| error::ErrorUnauthorized("Group not found"))?;

        if !group
            .compute_permissions(user.id, Some(message.to))
            .contains(Permissions::VIEW_MESSAGES)
        {
            return Err(error::ErrorUnauthorized(
                "You don't have permission to overwrite this message",
            ));
        }
    }

    message.data = data;

    state
        .messages
        .overwrite(message)
        .map_err(ErrorInternalServerError)
}

async fn delete_message(
    state: State,
    user: web::ReqData<JwtUser>,
    MsgPack(message_id): MsgPack<snowflake_id>,
) -> Result<(), Error> {
    let message = state
        .messages
        .get(message_id)
        .map_err(ErrorInternalServerError)?;

    let is_author = message.from == user.id;

    if let Some(group_id) = message.group_id {
        let group = state
            .groups
            .get(&group_id)
            .ok_or_else(|| error::ErrorUnauthorized("Group not found"))?;

        let perms = group.compute_permissions(user.id, Some(message.to));

        let can_view = perms.contains(Permissions::VIEW_MESSAGES);
        let can_manage = perms.contains(Permissions::MANAGE_MESSAGES);

        if !(can_manage || (can_view && is_author)) {
            return Err(error::ErrorUnauthorized(
                "You don't have permission to delete this message",
            ));
        }
    } else if !is_author {
        return Err(error::ErrorUnauthorized(
            "You are not the author of this message",
        ));
    }

    state
        .messages
        .delete(message.id)
        .map_err(ErrorInternalServerError)
}

async fn remove_dm(
    state: State,
    user: web::ReqData<JwtUser>,
    MsgPack(user_id): MsgPack<id>,
) -> Result<(), Error> {
    state
        .messages
        .remove_dm(user.id, user_id)
        .map_err(ErrorInternalServerError)
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/message")
            .route("/direct", web::get().to(get_messages))
            .route("/channel", web::get().to(get_channel_messages))
            .route("/overwrite", web::post().to(overwrite_message))
            .route("/delete", web::post().to(delete_message))
            .route("/remove_dm", web::post().to(remove_dm)),
    );
}
