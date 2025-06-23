use crate::models::{Message, id};
use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use chrono::{DateTime, Utc};
use rand_core::OsRng;
use serde::Serialize;
use sqlx::{Pool, Postgres};
use uuid::Uuid;

pub async fn init_db(pool: Pool<Postgres>) -> Result<(), sqlx::Error> {
    sqlx::query!(r#"CREATE EXTENSION IF NOT EXISTS "uuid-ossp";"#)
        .execute(&pool)
        .await?;

    sqlx::query!(
        "CREATE TABLE IF NOT EXISTS users (
            id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
            avatar TEXT,
            name TEXT NOT NULL,
            username TEXT NOT NULL UNIQUE,
            email TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        );"
    )
    .execute(&pool)
    .await?;

    sqlx::query!(
        "CREATE TABLE IF NOT EXISTS groups (
            id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
            avatar TEXT,
            name TEXT NOT NULL,
            users UUID[] NOT NULL DEFAULT '{}',
            voicechats UUID[] NOT NULL DEFAULT '{}',
            chats UUID[] NOT NULL DEFAULT '{}',
            admin UUID[] NOT NULL DEFAULT '{}',
            description TEXT,
            created_by UUID NOT NULL REFERENCES users(id),
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        );"
    )
    .execute(&pool)
    .await?;

    sqlx::query!(
        r#"CREATE TABLE IF NOT EXISTS messages (
            id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
            "from" UUID NOT NULL REFERENCES users(id),
            "to" UUID NOT NULL REFERENCES users(id),
            data JSONB NOT NULL,
            time TIMESTAMPTZ NOT NULL DEFAULT now(),
            type TEXT NOT NULL
        );"#
    )
    .execute(&pool)
    .await?;

    sqlx::query!(
        "CREATE TABLE IF NOT EXISTS voicechats (
            id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
            name TEXT NOT NULL
        );"
    )
    .execute(&pool)
    .await?;

    sqlx::query!(
        "CREATE TABLE IF NOT EXISTS chats (
            id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
            name TEXT NOT NULL
        );"
    )
    .execute(&pool)
    .await?;

    sqlx::query!(
        "CREATE TABLE IF NOT EXISTS friends (
            user_1 UUID NOT NULL REFERENCES users(id),
            user_2 UUID NOT NULL REFERENCES users(id),
            PRIMARY KEY (user_1, user_2),
            CHECK (user_1 < user_2)
        );"
    )
    .execute(&pool)
    .await?;

    sqlx::query!(
        r#"
        CREATE TABLE IF NOT EXISTS friend_requests (
            "from" UUID NOT NULL REFERENCES users(id),
            "to" UUID NOT NULL REFERENCES users(id),
            PRIMARY KEY ("from", "to")
        );
        "#
    )
    .execute(&pool)
    .await?;

    Ok(())
}

#[derive(Serialize, sqlx::FromRow, Default)]
pub struct UserDB {
    pub id: id,
    pub avatar: Option<String>,
    pub name: String,
    pub username: String,
    pub email: String,
    #[serde(skip_serializing)]
    pub password_hash: Option<String>,
    #[serde(skip_serializing)]
    pub created_at: Option<DateTime<Utc>>,
}

pub async fn register(
    pool: &Pool<Postgres>,
    username: &str,
    name: &str,
    email: &str,
    password: &str,
) -> Result<UserDB, sqlx::Error> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();

    let password_hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| sqlx::Error::Protocol(format!("Hash error: {}", e).into()))?
        .to_string();

    let user = sqlx::query_as!(
        UserDB,
        r#"
        INSERT INTO users (username, name, email, password_hash)
        VALUES ($1, $2, $3, $4)
        RETURNING id, avatar, name, username, email, password_hash, created_at
        "#,
        username,
        name,
        email,
        password_hash,
    )
    .fetch_one(pool)
    .await?;

    Ok(user)
}

pub async fn login(pool: &Pool<Postgres>, username: &str, password: &str) -> Option<UserDB> {
    let user = sqlx::query_as!(
        UserDB,
        r#"
        SELECT * FROM users
        WHERE username = $1
        "#,
        username
    )
    .fetch_optional(pool)
    .await
    .unwrap()?;

    let hash_str = user.password_hash.clone().unwrap();
    let parsed_hash = PasswordHash::new(&hash_str).ok()?;

    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .ok()?;

    Some(user)
}

pub async fn create_group(
    pool: &Pool<Postgres>,
    name: &str,
    description: Option<&str>,
    creator_id: Uuid,
) -> Result<Uuid, sqlx::Error> {
    let rec = sqlx::query!(
        r#"
        INSERT INTO groups (name, description, created_by)
        VALUES ($1, $2, $3)
        RETURNING id
        "#,
        name,
        description,
        creator_id
    )
    .fetch_one(pool)
    .await?;

    Ok(rec.id)
}

pub async fn save_message(pool: &Pool<Postgres>, message: &Message) -> Result<Uuid, sqlx::Error> {
    let msg_type: &str = message.r#type.clone().into();
    let rec = sqlx::query!(
        r#"
        INSERT INTO messages ("from", "to", data, "type")
        VALUES ($1, $2, $3, $4)
        RETURNING id
        "#,
        message.from,
        message.to,
        message.data,
        msg_type
    )
    .fetch_one(pool)
    .await?;

    Ok(rec.id)
}

pub async fn get_group_users(
    pool: &Pool<Postgres>,
    group_id: Uuid,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let rec = sqlx::query!(
        r#"
        SELECT users FROM groups WHERE id = $1
        "#,
        group_id
    )
    .fetch_one(pool)
    .await?;

    Ok(rec.users)
}

pub async fn get_friends(pool: &Pool<Postgres>, user_id: Uuid) -> Result<Vec<UserDB>, sqlx::Error> {
    let friends = sqlx::query_as!(
        UserDB,
        r#"
        SELECT u.*
        FROM users u
        WHERE u.id IN (
            SELECT user_2 FROM friends WHERE user_1 = $1
            UNION
            SELECT user_1 FROM friends WHERE user_2 = $1
        )
        "#,
        user_id
    )
    .fetch_all(pool)
    .await?;

    Ok(friends)
}

#[derive(PartialEq)]
pub enum AddFriend {
    Request,
    Add,
}

pub async fn add_friend(
    pool: &Pool<Postgres>,
    user_id: Uuid,
    friend_id: Uuid,
) -> Result<AddFriend, sqlx::Error> {
    let mut tx = pool.begin().await?;

    let request_exists = sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM friend_requests WHERE \"from\" = $1 AND \"to\" = $2)",
        friend_id,
        user_id
    )
    .fetch_one(&mut *tx)
    .await?
    .unwrap_or(false);

    if request_exists {
        let (user_1, user_2) = sort_pair(user_id, friend_id);

        sqlx::query!(
            "INSERT INTO friends (user_1, user_2) VALUES ($1, $2)",
            user_1,
            user_2
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            "DELETE FROM friend_requests WHERE \"from\" = $1 AND \"to\" = $2",
            friend_id,
            user_id
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        return Ok(AddFriend::Add);
    }

    sqlx::query!(
        "INSERT INTO friend_requests (\"from\", \"to\") VALUES ($1, $2)",
        user_id,
        friend_id
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(AddFriend::Request)
}

pub async fn are_friends(
    pool: &Pool<Postgres>,
    user_1: Uuid,
    user_2: Uuid,
) -> Result<bool, sqlx::Error> {
    let (user_1, user_2) = sort_pair(user_1, user_2);
    let exists = sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM friends WHERE user_1 = $1 AND user_2 = $2)",
        user_1,
        user_2
    )
    .fetch_one(pool)
    .await?
    .unwrap_or(false);

    Ok(exists)
}

#[derive(Serialize)]
pub struct Gruplist {
    id: Uuid,
    avatar: Option<String>,
    name: String,
    description: Option<String>,
}

pub async fn get_groups(
    pool: &Pool<Postgres>,
    user_id: Uuid,
) -> Result<Vec<Gruplist>, sqlx::Error> {
    let groups = sqlx::query_as!(
        Gruplist,
        r#"
        SELECT id, name, description, avatar
        FROM groups
        WHERE $1 = ANY(users);
        "#,
        user_id
    )
    .fetch_all(pool)
    .await?;

    Ok(groups)
}

pub async fn in_group(
    pool: &Pool<Postgres>,
    user_id: Uuid,
    group_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query!(
        r#"
        SELECT 1 AS exists FROM groups
        WHERE id = $1 AND $2 = ANY(users)
        "#,
        group_id,
        user_id
    )
    .fetch_optional(pool)
    .await?;

    Ok(result.is_some())
}

pub async fn get_messages(
    pool: &Pool<Postgres>,
    user_1: Uuid,
    user_2: Uuid,
    len: i64,
    end: Option<DateTime<Utc>>,
) -> Result<Vec<Message>, sqlx::Error> {
    let end_time = end.unwrap_or(Utc::now());

    let messages = sqlx::query_as!(
        Message,
        r#"
        SELECT * FROM (
            SELECT * FROM messages
            WHERE 
                (("from" = $1 AND "to" = $2) OR
                ("from" = $2 AND "to" = $1))
                AND time < $3
            ORDER BY time DESC
            LIMIT $4
        ) sub
        ORDER BY time ASC
        "#,
        user_1,
        user_2,
        end_time,
        len
    )
    .fetch_all(pool)
    .await?;

    Ok(messages)
}

pub async fn get_user(pool: &Pool<Postgres>, user_id: Uuid) -> Option<UserDB> {
    let user = sqlx::query_as!(
        UserDB,
        r#"
        SELECT * FROM users
        WHERE id = $1
        "#,
        user_id
    )
    .fetch_optional(pool)
    .await
    .ok()?;

    user
}

pub async fn get_users_like(
    pool: &Pool<Postgres>,
    username: &str,
) -> Result<Vec<UserDB>, sqlx::Error> {
    let pattern = format!("{}%", username);

    let users = sqlx::query_as!(
        UserDB,
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

pub async fn has_registered(
    pool: &Pool<Postgres>,
    username: &str,
    email: &str,
) -> Result<bool, sqlx::Error> {
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

fn sort_pair(a: Uuid, b: Uuid) -> (Uuid, Uuid) {
    if a < b { (a, b) } else { (b, a) }
}
