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
    let gid = *group_id as i64;

    let group = sqlx::query!(r#"SELECT id, icon, name FROM groups WHERE id = $1"#, gid)
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
        gid
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

    let mut channel_overrides: HashMap<i64, Vec<PermissionOverride>> = sqlx::query!(
        r#"
            SELECT * 
            FROM permission_overrides 
            WHERE group_id = $1
        "#,
        gid
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .fold(
        HashMap::<i64, Vec<PermissionOverride>>::new(),
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
        sqlx::query!(r#"SELECT * FROM channels WHERE group_id = $1"#, gid)
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(|row| {
                let ch_id = id::from(row.id);
                let c_type = if row.channel_type {
                    ChannelType::Voice
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
        gid,
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
        id::from(0_u64),
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
        *creator_id as i64
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
        *group_id as i64,
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
        &group_ids
            .iter()
            .map(|i| i64::from(*i))
            .collect::<Vec<i64>>()
    )
    .fetch_all(pool)
    .await
}

/* ===== CHANNEL ===== */

pub async fn create_channel(
    pool: &Pool<Postgres>,
    group_id: id,
    name: String,
    r#type: ChannelType,
) -> Result<id, sqlx::Error> {
    let is_voice = matches!(r#type, ChannelType::Voice);

    let id = sqlx::query_scalar!(
        r#"
        INSERT INTO channels (group_id, name, position, channel_type)
        VALUES (
            $1, 
            $2, 
            (SELECT COUNT(*) FROM channels WHERE group_id = $1) + 1, 
            $3
        )
        RETURNING id as "id:id"
        "#,
        *group_id as i64,
        name,
        is_voice
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
            *channel_id as i64
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
                *group_id as i64,
                *channel_id as i64,
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
                *group_id as i64,
                *channel_id as i64,
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
        SET name = COALESCE($2, name), title = COALESCE($3, title), position = COALESCE($4, position)
        WHERE id = $1
        "#,
        *channel_id as i64,
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
        *group_id as i64,
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
    role_id: id,
    name: Option<String>,
    color: Option<String>,
    permissions: Option<u64>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE roles 
            SET name = COALESCE($2, name), 
            color = COALESCE($3, color),
            permissions = COALESCE($4, permissions)
        WHERE id = $1
        "#,
        *role_id as i64,
        name,
        color,
        permissions.map(|p| p as i64)
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
        *role_id as i64,
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
        *user_id as i64,
        *role_id as i64,
        *group_id as i64,
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
        *user_id as i64,
        *group_id as i64,
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn get_members(pool: &Pool<Postgres>, group_id: id) -> Result<Vec<id>, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT user_id FROM group_users WHERE group_id = $1",
        *group_id as i64,
    )
    .fetch_all(pool)
    .await
    .map(|ids| ids.into_iter().map(|user_id| id::from(user_id)).collect())
}
