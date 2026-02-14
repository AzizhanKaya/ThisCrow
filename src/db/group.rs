use crate::id::id;
use crate::state::group::Group;
use anyhow::Result;
use sqlx::{Pool, Postgres};

pub async fn init_group(pool: &Pool<Postgres>, group_id: id) -> Result<Group, sqlx::Error> {
    todo!("init_group");
}

pub async fn create_group(
    pool: &Pool<Postgres>,
    name: &str,
    description: Option<&str>,
    creator_id: id,
) -> Result<id, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        INSERT INTO groups (name, description, created_by)
        VALUES ($1, $2, $3)
        RETURNING id as "id: id"
        "#,
        name,
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
    icon: Option<String>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE groups 
        SET name = COALESCE($2, name), icon = COALESCE($3, icon)
        WHERE id = $1
        "#,
        *group_id as i64,
        name,
        icon
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn get_group_users(
    pool: &Pool<Postgres>,
    group_id: i64,
) -> Result<Vec<u64>, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT user_id FROM group_users WHERE group_id = $1",
        group_id
    )
    .fetch_all(pool)
    .await
    .map(|ids| ids.into_iter().map(|user_id| user_id as u64).collect())
}

pub async fn get_groups(pool: &Pool<Postgres>, user_id: id) -> Result<Vec<id>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT 
            g.id as "id: id"
        FROM groups g
        JOIN group_users gu ON g.id = gu.group_id
        WHERE gu.user_id = $1
        "#,
        *user_id as i64
    )
    .fetch_all(pool)
    .await
}

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
