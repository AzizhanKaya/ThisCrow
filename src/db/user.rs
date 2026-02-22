use anyhow::Result;
use anyhow::anyhow;
use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use chrono::{DateTime, Utc};
use rand_core::OsRng;
use serde::Serialize;
use sqlx::{Pool, Postgres};

type id = crate::id::id;

#[derive(Serialize, sqlx::FromRow, Default, Clone)]
pub struct User {
    pub id: id,
    pub avatar: Option<String>,
    pub name: String,
    pub username: String,
    #[serde(skip_serializing, skip_deserializing)]
    pub email: String,
    #[serde(skip_serializing, skip_deserializing)]
    pub password_hash: String,
    #[serde(skip_serializing, skip_deserializing)]
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing, skip_deserializing)]
    pub last_seen: Option<DateTime<Utc>>,
}

pub async fn login(pool: &Pool<Postgres>, username: &str, password: &str) -> Option<User> {
    let user = sqlx::query_as!(
        User,
        r#"
        SELECT * FROM users
        WHERE username = $1
        "#,
        username
    )
    .fetch_optional(pool)
    .await
    .ok()??;

    let parsed_hash = PasswordHash::new(&user.password_hash).ok()?;

    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .ok()?;

    Some(user)
}

pub async fn register(
    pool: &Pool<Postgres>,
    username: &str,
    name: &str,
    email: &str,
    password: &str,
) -> Result<User> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();

    let password_hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow!("Hash error: {}", e))?;

    let user = sqlx::query_as!(
        User,
        r#"
        INSERT INTO users (username, name, email, password_hash)
        VALUES ($1, $2, $3, $4)
        RETURNING *
        "#,
        username,
        name,
        email,
        password_hash.to_string(),
    )
    .fetch_one(pool)
    .await?;

    Ok(user)
}

pub async fn has_registered(pool: &Pool<Postgres>, username: &str, email: &str) -> Result<bool> {
    let exists: Option<bool> = sqlx::query_scalar!(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM users WHERE email = $1 OR username = $2
        )
        "#,
        email,
        username
    )
    .fetch_one(pool)
    .await?;

    Ok(exists.unwrap_or(false))
}

pub async fn get_friends(pool: &Pool<Postgres>, user_id: id) -> Result<Vec<id>, sqlx::Error> {
    let friends = sqlx::query_scalar!(
        r#"
        SELECT u.id AS "id:id"
        FROM users u    
        WHERE u.id IN (
            SELECT user_2 FROM friends WHERE user_1 = $1
            UNION
            SELECT user_1 FROM friends WHERE user_2 = $1
        )
        "#,
        *user_id as i64
    )
    .fetch_all(pool)
    .await;

    friends
}

pub async fn friend_requests(pool: &Pool<Postgres>, user_id: id) -> Result<Vec<id>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT u.id AS "id:id"
        FROM users u
        JOIN friend_requests fr ON fr."from" = u.id
        WHERE fr."to" = $1
        "#,
        *user_id as i64
    )
    .fetch_all(pool)
    .await
}

pub async fn outgoing_friend_requests(
    pool: &Pool<Postgres>,
    user_id: id,
) -> Result<Vec<id>, sqlx::Error> {
    let users: Vec<id> = sqlx::query_scalar!(
        r#"
        SELECT u.id AS "id:id"
        FROM users u
        JOIN friend_requests fr ON fr."to" = u.id
        WHERE fr."from" = $1
        "#,
        *user_id as i64
    )
    .fetch_all(pool)
    .await?;

    Ok(users)
}

pub async fn friend_request(pool: &Pool<Postgres>, from: id, to: id) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO friend_requests ("from", "to")
        VALUES ($1, $2)
        ON CONFLICT DO NOTHING
        "#,
        *from as i64,
        *to as i64
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn friend_accept(
    pool: &Pool<Postgres>,
    user_id: id,
    friend_id: id,
) -> Result<bool, sqlx::Error> {
    let request_exists = sqlx::query_scalar!(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM friend_requests
            WHERE "from" = $1 AND "to" = $2
        )
        "#,
        *friend_id as i64,
        *user_id as i64
    )
    .fetch_one(pool)
    .await?
    .unwrap_or(false);

    if !request_exists {
        return Ok(false);
    }

    let (user_1, user_2) = user_id.sort_pair(friend_id);

    let mut tx = pool.begin().await?;

    sqlx::query!(
        r#"
        INSERT INTO friends (user_1, user_2)
        VALUES ($1, $2)
        ON CONFLICT DO NOTHING
        "#,
        *user_1 as i64,
        *user_2 as i64
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query!(
        r#"
        DELETE FROM friend_requests
        WHERE "from" = $1 AND "to" = $2
        "#,
        *friend_id as i64,
        *user_id as i64
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(true)
}

pub async fn friend_remove(pool: &Pool<Postgres>, from: id, to: id) -> Result<(), sqlx::Error> {
    let (user_1, user_2) = from.sort_pair(to);

    let mut tx = pool.begin().await?;

    sqlx::query!(
        r#"
        DELETE FROM friends
        WHERE user_1 = $1 AND user_2 = $2
        "#,
        *user_1 as i64,
        *user_2 as i64
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query!(
        r#"
        DELETE FROM friend_requests
        WHERE "from" = $1 AND "to" = $2
        "#,
        *from as i64,
        *to as i64
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

pub async fn are_friends(
    pool: &Pool<Postgres>,
    user_1: id,
    user_2: id,
) -> Result<bool, sqlx::Error> {
    let (user_1, user_2) = user_1.sort_pair(user_2);
    let exists = sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM friends WHERE user_1 = $1 AND user_2 = $2)",
        *user_1 as i64,
        *user_2 as i64
    )
    .fetch_one(pool)
    .await?
    .unwrap_or(false);

    Ok(exists)
}

pub async fn get_user(pool: &Pool<Postgres>, user_id: id) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as!(
        User,
        r#"
        SELECT * FROM users
        WHERE id = $1
        "#,
        *user_id as i64
    )
    .fetch_optional(pool)
    .await
}

pub async fn get_user_by_username(
    pool: &Pool<Postgres>,
    username: &str,
) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as!(
        User,
        r#"
        SELECT * FROM users
        WHERE username = $1
        "#,
        username
    )
    .fetch_optional(pool)
    .await
}

pub async fn get_users_like(
    pool: &Pool<Postgres>,
    username: &str,
) -> Result<Vec<User>, sqlx::Error> {
    let pattern = format!("{}%", username);

    let users = sqlx::query_as!(
        User,
        r#"
        SELECT * FROM users
        WHERE username LIKE $1
        LIMIT 10
        "#,
        pattern
    )
    .fetch_all(pool)
    .await?;

    Ok(users)
}

pub async fn update_last_seen(
    pool: &Pool<Postgres>,
    user_id: id,
    time: DateTime<Utc>,
) -> Result<()> {
    sqlx::query!(
        "UPDATE users SET last_seen = $1 WHERE id = $2",
        time,
        *user_id as i64
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn get_users_by_ids(
    pool: &Pool<Postgres>,
    users: Vec<id>,
) -> Result<Vec<User>, sqlx::Error> {
    if users.is_empty() {
        return Ok(Vec::new());
    }

    let users: Vec<User> = sqlx::query_as!(
        User,
        r#"
        SELECT u.* FROM users u
        WHERE u.id = ANY($1)
        "#,
        &users.iter().map(|id| i64::from(*id)).collect::<Vec<i64>>()
    )
    .fetch_all(pool)
    .await?;

    Ok(users)
}

pub async fn update_user(
    pool: &Pool<Postgres>,
    user_id: id,
    name: Option<String>,
    avatar: Option<String>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE users 
        SET name = COALESCE($2, name), avatar = COALESCE($3, avatar)
        WHERE id = $1
        "#,
        *user_id as i64,
        name,
        avatar
    )
    .execute(pool)
    .await?;

    Ok(())
}
