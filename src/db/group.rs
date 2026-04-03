use std::collections::{HashMap, HashSet};

use crate::state::{
    self,
    group::{Channel, ChannelType, Member, OverrideTarget, PermissionOverride, Permissions, Role},
};
use anyhow::Result;
use sqlx::{Pool, Postgres};

type id = crate::id::id;

/* ===== GROUP ===== */

#[derive(sqlx::FromRow)]
pub struct Group {
    pub id: id,
    pub icon: Option<String>,
    pub name: String,
    pub description: Option<String>,
}

pub async fn init_group(pool: &Pool<Postgres>, group_id: id) -> Result<state::Group, sqlx::Error> {
    let group = sqlx::query!(
        r#"SELECT id, icon, name, created_by FROM groups WHERE id = $1"#,
        *group_id
    )
    .fetch_one(pool)
    .await?;

    let roles: HashMap<id, Role> = sqlx::query!(
        r#"
            SELECT 
                id AS "id:id", 
                name, 
                COALESCE(color, '') AS "color!",
                position,
                permissions 
            FROM roles
            WHERE group_id = $1
        "#,
        *group_id
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| {
        (
            r.id,
            Role::new(
                r.id,
                r.name,
                r.position as usize,
                r.color,
                Permissions::from_bits_truncate(r.permissions as u64),
            ),
        )
    })
    .collect();

    let mut channel_overrides: HashMap<i32, Vec<PermissionOverride>> = sqlx::query!(
        r#"
            SELECT * 
            FROM permission_overrides 
            WHERE group_id = $1
        "#,
        *group_id
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .fold(
        HashMap::<i32, Vec<PermissionOverride>>::new(),
        |mut acc, row| {
            let target = if let Some(r_id) = row.role_id {
                OverrideTarget::Role(id::from(r_id))
            } else if let Some(u_id) = row.user_id {
                OverrideTarget::User(id::from(u_id))
            } else {
                return acc;
            };

            acc.entry(row.channel_id)
                .or_default()
                .push(PermissionOverride::new(
                    target,
                    Permissions::from_bits_truncate(row.allow as u64),
                    Permissions::from_bits_truncate(row.deny as u64),
                ));
            acc
        },
    );

    let channels: HashMap<id, Channel> =
        sqlx::query!(r#"SELECT * FROM channels WHERE group_id = $1"#, *group_id)
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(|row| {
                let ch_id = id::from(row.id);
                let c_type = if row.channel_type {
                    ChannelType::Voice {
                        users: HashSet::new(),
                        watch_party: None,
                    }
                } else {
                    ChannelType::Text
                };

                (
                    ch_id,
                    Channel::new(
                        ch_id,
                        row.name,
                        row.title,
                        row.position as usize,
                        c_type,
                        channel_overrides.remove(&row.id).unwrap_or_default(),
                    ),
                )
            })
            .collect();

    let members: HashMap<id, Member> = sqlx::query!(
        r#"
            SELECT gu.user_id, gu.name, gur.role_id AS "role_id?"
            FROM group_users gu
            LEFT JOIN group_user_roles gur 
            ON gu.group_id = gur.group_id AND gu.user_id = gur.user_id
            WHERE gu.group_id = $1
        "#,
        *group_id,
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .fold(
        HashMap::<id, (Option<String>, Vec<id>)>::new(),
        |mut acc, row| {
            let uid = id::from(row.user_id);
            let entry = acc.entry(uid).or_insert((row.name, Vec::new()));

            if let Some(rid) = row.role_id {
                entry.1.push(id::from(rid));
            }

            acc
        },
    )
    .into_iter()
    .map(|(uid, (name, roles))| (uid, Member::new(uid, name, roles)))
    .collect();

    Ok(state::Group::new(
        id::from(group.id),
        group.icon,
        group.name,
        group_id,
        id::from(group.created_by),
        members,
        roles,
        channels,
        HashSet::new(),
    ))
}

pub async fn create_group(
    pool: &Pool<Postgres>,
    name: &str,
    icon: Option<&str>,
    description: Option<&str>,
    creator_id: id,
) -> Result<id, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        INSERT INTO groups (name, icon, description, created_by)
        VALUES ($1, $2, $3, $4)
        RETURNING id as "id: id"
        "#,
        name,
        icon,
        description,
        *creator_id
    )
    .fetch_one(pool)
    .await
}

pub async fn update_group(
    pool: &Pool<Postgres>,
    group_id: id,
    name: Option<String>,
    description: Option<String>,
    icon: Option<String>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE groups 
        SET name = COALESCE($2, name), description = COALESCE($3, description), icon = COALESCE($4, icon)
        WHERE id = $1
        "#,
        *group_id,
        name,
        description,
        icon
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn get_groups_info(
    pool: &Pool<Postgres>,
    group_ids: Vec<id>,
) -> Result<Vec<Group>, sqlx::Error> {
    if group_ids.is_empty() {
        return Ok(Vec::new());
    }

    sqlx::query_as!(
        Group,
        r#"
        SELECT 
            g.id,
            g.icon,
            g.name,
            g.description
        FROM groups g
        WHERE g.id = ANY($1)
        "#,
        &group_ids as _
    )
    .fetch_all(pool)
    .await
}

/* ===== CHANNEL ===== */

pub async fn create_channel(
    pool: &Pool<Postgres>,
    group_id: id,
    name: String,
    title: Option<String>,
    is_voice: bool,
) -> Result<id, sqlx::Error> {
    let id = sqlx::query_scalar!(
        r#"
        INSERT INTO channels (group_id, name, position, channel_type, title)
        VALUES (
            $1, 
            $2, 
            (SELECT COUNT(*) FROM channels WHERE group_id = $1) + 1, 
            $3,
            $4
        )
        RETURNING id as "id:id"
        "#,
        *group_id,
        name,
        is_voice,
        title
    )
    .fetch_one(pool)
    .await?;

    Ok(id)
}

pub async fn update_channel(
    pool: &Pool<Postgres>,
    group_id: id,
    channel_id: id,
    name: Option<String>,
    title: Option<String>,
    position: Option<usize>,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;

    if let Some(new_pos) = position {
        let new_pos = new_pos as i16;

        let old_pos = sqlx::query_scalar!(
            r#"
            SELECT position 
            FROM channels 
            WHERE id = $1
            "#,
            *channel_id
        )
        .fetch_one(&mut *tx)
        .await?;

        if new_pos > old_pos {
            sqlx::query!(
                r#"
                        UPDATE channels
                        SET position = position - 1
                        WHERE group_id = $1 AND id != $2 AND position > $3 AND position <= $4
                        "#,
                *group_id,
                *channel_id,
                old_pos,
                new_pos
            )
            .execute(&mut *tx)
            .await?;
        } else if new_pos < old_pos {
            sqlx::query!(
                r#"
                        UPDATE channels
                        SET position = position + 1
                        WHERE group_id = $1 AND id != $2 AND position >= $3 AND position < $4
                        "#,
                *group_id,
                *channel_id,
                new_pos,
                old_pos
            )
            .execute(&mut *tx)
            .await?;
        }
    }

    sqlx::query!(
        r#"
        UPDATE channels 
        SET name = COALESCE($2, name), title = $3, position = COALESCE($4, position)
        WHERE id = $1
        "#,
        *channel_id,
        name,
        title,
        position.map(|p| p as i16),
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(())
}

/* ===== ROLE ===== */

pub async fn create_role(
    pool: &Pool<Postgres>,
    group_id: id,
    name: String,
    color: String,
    permissions: u64,
) -> Result<id, sqlx::Error> {
    let rec = sqlx::query!(
        r#"
        INSERT INTO roles (group_id, name, color, permissions)
        VALUES ($1, $2, $3, $4)
        RETURNING id as "id:id"
        "#,
        *group_id,
        name,
        color,
        permissions as i64
    )
    .fetch_one(pool)
    .await?;

    Ok(rec.id)
}

pub async fn update_role(
    pool: &Pool<Postgres>,
    group_id: id,
    role_id: id,
    name: Option<String>,
    color: Option<String>,
    permissions: Option<u64>,
    position: Option<usize>,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;

    if let Some(new_pos) = position {
        let new_pos = new_pos as i16;

        let old_pos = sqlx::query_scalar!(
            r#"
            SELECT position 
            FROM roles 
            WHERE id = $1
            "#,
            *role_id
        )
        .fetch_one(&mut *tx)
        .await?;

        if new_pos > old_pos {
            sqlx::query!(
                r#"
                        UPDATE roles
                        SET position = position - 1
                        WHERE group_id = $1 AND id != $2 AND position > $3 AND position <= $4
                        "#,
                *group_id,
                *role_id,
                old_pos,
                new_pos
            )
            .execute(&mut *tx)
            .await?;
        } else if new_pos < old_pos {
            sqlx::query!(
                r#"
                        UPDATE roles
                        SET position = position + 1
                        WHERE group_id = $1 AND id != $2 AND position >= $3 AND position < $4
                        "#,
                *group_id,
                *role_id,
                new_pos,
                old_pos
            )
            .execute(&mut *tx)
            .await?;
        }
    }
    sqlx::query!(
        r#"
        UPDATE roles 
            SET name = COALESCE($2, name), 
            color = COALESCE($3, color),
            permissions = COALESCE($4, permissions),
            position = COALESCE($5, position)
        WHERE id = $1
        "#,
        *role_id,
        name,
        color,
        permissions.map(|p| p as i64),
        position.map(|p| p as i16)
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn delete_role(pool: &Pool<Postgres>, role_id: id) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        DELETE FROM roles 
        WHERE id = $1
        "#,
        *role_id,
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn assign_role(
    pool: &Pool<Postgres>,
    user_id: id,
    role_id: id,
    group_id: id,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO group_user_roles (user_id, role_id, group_id)
        VALUES ($1, $2, $3)
        "#,
        *user_id,
        *role_id,
        *group_id,
    )
    .execute(pool)
    .await?;

    Ok(())
}

/* ===== MEMBER ===== */

pub async fn add_member(
    pool: &Pool<Postgres>,
    user_id: id,
    group_id: id,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO group_users (user_id, group_id, position)
        VALUES (
            $1, 
            $2, 
            (SELECT COALESCE(MAX(position), 0) + 1 FROM group_users WHERE user_id = $1)
        )
        "#,
        *user_id,
        *group_id,
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn get_members(pool: &Pool<Postgres>, group_id: id) -> Result<Vec<id>, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT user_id FROM group_users WHERE group_id = $1",
        *group_id,
    )
    .fetch_all(pool)
    .await
    .map(|ids| ids.into_iter().map(|user_id| id::from(user_id)).collect())
}

pub async fn get_member_count(pool: &Pool<Postgres>, group_id: id) -> Result<i64, sqlx::Error> {
    let count = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM group_users WHERE group_id = $1",
        *group_id,
    )
    .fetch_one(pool)
    .await?
    .unwrap_or(0);

    Ok(count)
}

/* ===== INVITATION ===== */

#[derive(sqlx::FromRow, serde::Serialize)]
pub struct Invitation {
    pub id: id,
    pub code: String,
    pub group_id: id,
    pub created_by: id,
    pub max_uses: Option<i32>,
    pub uses: i32,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn create_invitation(
    pool: &Pool<Postgres>,
    code: &str,
    group_id: id,
    created_by: id,
    max_uses: Option<i32>,
    expires_at: chrono::DateTime<chrono::Utc>,
) -> Result<Invitation, sqlx::Error> {
    sqlx::query_as!(
        Invitation,
        r#"
        INSERT INTO invitations (code, group_id, created_by, max_uses, expires_at)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING *
        "#,
        code,
        *group_id,
        *created_by,
        max_uses,
        expires_at
    )
    .fetch_one(pool)
    .await
}

pub async fn get_invitation_by_code(
    pool: &Pool<Postgres>,
    code: &str,
) -> Result<Option<Invitation>, sqlx::Error> {
    sqlx::query_as!(
        Invitation,
        r#"
        SELECT *
        FROM invitations
        WHERE code = $1
        "#,
        code
    )
    .fetch_optional(pool)
    .await
}

pub async fn get_invitation_by_id(
    pool: &Pool<Postgres>,
    invitation_id: id,
) -> Result<Option<Invitation>, sqlx::Error> {
    sqlx::query_as!(
        Invitation,
        r#"
        SELECT *
        FROM invitations
        WHERE id = $1
        "#,
        *invitation_id
    )
    .fetch_optional(pool)
    .await
}

pub async fn increment_invitation_uses(
    pool: &Pool<Postgres>,
    invitation_id: id,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE invitations SET uses = uses + 1 WHERE id = $1",
        *invitation_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_group_invitations(
    pool: &Pool<Postgres>,
    group_id: id,
) -> Result<Vec<Invitation>, sqlx::Error> {
    sqlx::query_as!(
        Invitation,
        r#"
        SELECT *
        FROM invitations
        WHERE group_id = $1 AND expires_at > now()
        ORDER BY created_at DESC
        "#,
        *group_id
    )
    .fetch_all(pool)
    .await
}

pub async fn delete_invitation(
    pool: &Pool<Postgres>,
    invitation_id: id,
) -> Result<(), sqlx::Error> {
    sqlx::query!("DELETE FROM invitations WHERE id = $1", *invitation_id)
        .execute(pool)
        .await?;
    Ok(())
}
