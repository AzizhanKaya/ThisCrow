use crate::id::id;
use crate::state::group::Permissions;
use crate::{State, db::message::StoredMessage, middleware::JwtUser, msgpack::MsgPack};
use actix_web::error;
use actix_web::{Error, error::ErrorInternalServerError, web};

async fn overwrite_message(
    state: State,
    user: web::ReqData<JwtUser>,
    MsgPack(mut message): MsgPack<StoredMessage>,
) -> Result<(), Error> {
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

    state
        .messages
        .overwrite(message)
        .map_err(ErrorInternalServerError)
}

async fn delete_message(
    state: State,
    user: web::ReqData<JwtUser>,
    MsgPack(message): MsgPack<StoredMessage>,
) -> Result<(), Error> {
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

async fn delete_dm(
    state: State,
    user: web::ReqData<JwtUser>,
    MsgPack(user_id): MsgPack<id>,
) -> Result<(), Error> {
    state
        .messages
        .delete_dm(user.id, user_id)
        .map_err(ErrorInternalServerError)
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/message")
            .route("/overwrite", web::post().to(overwrite_message))
            .route("/delete", web::post().to(delete_message))
            .route("/delete_dm", web::post().to(delete_dm)),
    );
}
