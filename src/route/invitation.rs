use crate::db::group::Invitation;
use crate::id::id;
use crate::message::Ack;
use crate::message::Message;
use crate::middleware::JwtUser;
use crate::msgpack::MsgPack;
use crate::state::group::Member;
use crate::state::group::Permissions;
use crate::{State, db};
use actix_web::{Error, HttpResponse, error, web};
use chrono::{DateTime, Duration, Utc};
use rand::{Rng, distr::Alphanumeric};
use serde::Deserialize;

#[derive(Deserialize)]
struct CreateInvitationRequest {
    group_id: id,
    max_uses: Option<i32>,
    expires_at: Option<DateTime<Utc>>,
}

async fn create_invitation(
    state: State,
    user: web::ReqData<JwtUser>,
    MsgPack(req): MsgPack<CreateInvitationRequest>,
) -> Result<MsgPack<Invitation>, Error> {
    if let Some(group) = state.groups.get(&req.group_id) {
        if !group
            .compute_permissions(user.id, None)
            .contains(Permissions::CREATE_INVITE)
        {
            return Err(error::ErrorForbidden(
                "You don't have permission to create an invitation",
            ));
        }
    }

    let code: String = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(8)
        .map(char::from)
        .collect();

    let expires_at = req
        .expires_at
        .unwrap_or_else(|| Utc::now() + Duration::try_days(7).unwrap());

    let invitation = db::group::create_invitation(
        &state.pool,
        &code,
        req.group_id,
        user.id,
        req.max_uses,
        expires_at,
    )
    .await
    .map_err(|e| {
        log::error!("Error while creating invitation: {}", e);
        error::ErrorInternalServerError("Error while creating invitation")
    })?;

    Ok(MsgPack(invitation))
}

async fn join_invitation(
    state: State,
    user: web::ReqData<JwtUser>,
    MsgPack(code): MsgPack<String>,
) -> Result<HttpResponse, Error> {
    let invitation = db::group::get_invitation_by_code(&state.pool, &code)
        .await
        .map_err(|e| {
            log::error!("Error while getting invitation by code: {}", e);
            error::ErrorInternalServerError("Error while getting invitation by code")
        })?
        .ok_or_else(|| error::ErrorNotFound("Invitation code not found"))?;

    if invitation.expires_at < Utc::now() {
        return Err(error::ErrorGone("Invitation expired"));
    }

    if let Some(max_uses) = invitation.max_uses {
        if invitation.uses >= max_uses {
            return Err(error::ErrorGone("Invitation has reached max uses"));
        }
    }

    db::group::add_member(&state.pool, user.id, invitation.group_id)
        .await
        .map_err(|_| error::ErrorConflict("You are already a member of this group"))?;

    db::group::increment_invitation_uses(&state.pool, invitation.id)
        .await
        .map_err(|e| {
            log::warn!("Error increment invitation uses: {}", e);
            error::ErrorInternalServerError("Error increment invitation uses")
        })?;

    if let Some(mut user) = state.users.get_mut(&user.id) {
        user.state.groups.push(invitation.group_id);
    }

    if let Some(mut group) = state.groups.get_mut(&invitation.group_id) {
        group
            .members
            .insert(user.id, Member::new(user.id, None, vec![]));

        group.notify(
            Message {
                from: group.id,
                to: user.id,
                data: Ack::JoinedMember,
                ..Default::default()
            },
            &state,
        );
    }

    Ok(HttpResponse::Ok().finish())
}

async fn delete_invitation(
    state: State,
    user: web::ReqData<JwtUser>,
    MsgPack(invitation_id): MsgPack<id>,
) -> Result<HttpResponse, Error> {
    let invitation = db::group::get_invitation_by_id(&state.pool, invitation_id)
        .await
        .map_err(|e| {
            log::error!("Error while getting invitation by id: {}", e);
            error::ErrorInternalServerError("Error while getting invitation by id")
        })?
        .ok_or_else(|| error::ErrorNotFound("Invitation code not found"))?;

    if let Some(group) = state.groups.get(&invitation.group_id) {
        if !group
            .compute_permissions(user.id, None)
            .contains(Permissions::DELETE_INVITE)
        {
            return Err(error::ErrorForbidden(
                "You don't have permission to delete an invitation",
            ));
        }
    }

    db::group::delete_invitation(&state.pool, invitation_id)
        .await
        .map_err(|e| {
            log::error!("Error delete_invitation: {}", e);
            error::ErrorInternalServerError("Error delete_invitation")
        })?;

    Ok(HttpResponse::Ok().finish())
}

use serde::Serialize;

#[derive(Deserialize)]
struct InvitationInfoQuery {
    code: String,
}

#[derive(Serialize)]
struct InvitationInfo {
    group_id: id,
    group_name: String,
    group_icon: Option<String>,
    group_description: Option<String>,
    member_count: i64,
}

async fn invitation_info(
    state: State,
    query: web::Query<InvitationInfoQuery>,
) -> Result<MsgPack<InvitationInfo>, Error> {
    let invitation = db::group::get_invitation_by_code(&state.pool, &query.code)
        .await
        .map_err(|e| {
            log::error!("Error while getting invitation by code: {}", e);
            error::ErrorInternalServerError("Error while getting invitation by code")
        })?
        .ok_or_else(|| error::ErrorNotFound("Invitation code not found"))?;

    if invitation.expires_at < Utc::now() {
        return Err(error::ErrorGone("Invitation expired"));
    }

    if let Some(max_uses) = invitation.max_uses {
        if invitation.uses >= max_uses {
            return Err(error::ErrorGone("Invitation has reached max uses"));
        }
    }

    let group = db::group::get_groups_info(&state.pool, vec![invitation.group_id])
        .await
        .map_err(|e| {
            log::error!("Error while getting group info: {}", e);
            error::ErrorInternalServerError("Error while getting group info")
        })?
        .into_iter()
        .next()
        .ok_or_else(|| error::ErrorNotFound("Group not found"))?;

    let member_count = db::group::get_member_count(&state.pool, invitation.group_id)
        .await
        .map_err(|e| {
            log::error!("Error while getting member count: {}", e);
            error::ErrorInternalServerError("Error while getting member count")
        })?;

    Ok(MsgPack(InvitationInfo {
        group_id: invitation.group_id,
        group_name: group.name,
        group_icon: group.icon,
        group_description: group.description,
        member_count,
    }))
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/invitation")
            .route("/create", web::post().to(create_invitation))
            .route("/join", web::post().to(join_invitation))
            .route("/delete", web::post().to(delete_invitation))
            .route("/info", web::get().to(invitation_info)),
    );
}
