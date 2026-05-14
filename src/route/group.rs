use crate::db::group::BannedUser;
use crate::id::id;
use crate::middleware::JwtUser;
use crate::msgpack::MsgPack;
use crate::state::group::Permissions;
use crate::{State, db};
use actix_web::{Error, HttpResponse, error, web};

fn require_ban_perm(state: &State, group_id: id, user_id: id) -> Result<(), Error> {
    let group = state
        .groups
        .get(&group_id)
        .ok_or_else(|| error::ErrorNotFound("Group not found"))?;

    if !group
        .compute_permissions(user_id, None)
        .intersects(Permissions::BAN_MEMBERS | Permissions::ADMINISTRATOR)
    {
        return Err(error::ErrorForbidden(
            "You don't have permission to manage bans",
        ));
    }

    Ok(())
}

async fn list_bans(
    state: State,
    user: web::ReqData<JwtUser>,
    path: web::Path<id>,
) -> Result<MsgPack<Vec<BannedUser>>, Error> {
    let group_id = path.into_inner();

    require_ban_perm(&state, group_id, user.id)?;

    let bans = db::group::get_bans(&state.pool, group_id)
        .await
        .map_err(|e| {
            log::error!("Error list_bans: {}", e);
            error::ErrorInternalServerError("Error list_bans")
        })?;

    Ok(MsgPack(bans))
}

async fn unban(
    state: State,
    user: web::ReqData<JwtUser>,
    path: web::Path<(id, id)>,
) -> Result<HttpResponse, Error> {
    let (group_id, target) = path.into_inner();

    require_ban_perm(&state, group_id, user.id)?;

    let _lock = state.group_locks.write(group_id).await;

    let removed = db::group::unban_user(&state.pool, group_id, target)
        .await
        .map_err(|e| {
            log::error!("Error unban_user: {}", e);
            error::ErrorInternalServerError("Error unban_user")
        })?;

    if !removed {
        return Err(error::ErrorNotFound("User is not banned"));
    }

    if let Some(mut group) = state.groups.get_mut(&group_id) {
        group.remove_ban(target);
    }

    Ok(HttpResponse::NoContent().finish())
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/group")
            .route("/{group_id}/bans", web::get().to(list_bans))
            .route("/{group_id}/bans/{user_id}", web::delete().to(unban)),
    );
}
