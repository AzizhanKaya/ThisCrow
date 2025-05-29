use crate::models::MessageType;
use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use rand_core::OsRng;
use serde_json::Value;
use sqlx::{Pool, Postgres};
use uuid::Uuid;

pub async fn init_db(pool: Pool<Postgres>) -> Result<(), sqlx::Error> {
    sqlx::query!(r#"CREATE EXTENSION IF NOT EXISTS "uuid-ossp";"#)
        .execute(&pool)
        .await?;

    sqlx::query!(
        "CREATE TABLE IF NOT EXISTS users (
            id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
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
        "CREATE TABLE IF NOT EXISTS messages (
            id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
            from_user UUID NOT NULL REFERENCES users(id),
            to_user UUID NOT NULL REFERENCES users(id),
            data JSONB NOT NULL,
            time TIMESTAMPTZ NOT NULL DEFAULT now(),
            type TEXT NOT NULL
        );"
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

    Ok(())
}

pub async fn register(
    pool: &Pool<Postgres>,
    username: &str,
    email: &str,
    password: &str,
) -> Result<Uuid, sqlx::Error> {
    let salt = SaltString::generate(&mut OsRng);

    let argon2 = Argon2::default();
    let password_hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| sqlx::Error::Protocol(format!("Hash error: {}", e).into()))?
        .to_string();

    let rec = sqlx::query!(
        "
        INSERT INTO users (username, email, password_hash)
        VALUES ($1, $2, $3)
        RETURNING id
        ",
        username,
        email,
        password_hash,
    )
    .fetch_one(pool)
    .await?;

    Ok(rec.id)
}

pub async fn login(pool: &Pool<Postgres>, username: &str, password: &str) -> Option<Uuid> {
    let user = sqlx::query!(
        r#"SELECT id, password_hash FROM users WHERE username = $1"#,
        username
    )
    .fetch_optional(pool)
    .await
    .unwrap()?;

    let parsed_hash = PasswordHash::new(&user.password_hash).ok()?;

    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .ok();

    Some(user.id)
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

pub async fn save_message(
    pool: &Pool<Postgres>,
    from_user: Uuid,
    to_user: Uuid,
    data: Value,
    msg_type: MessageType,
) -> Result<Uuid, sqlx::Error> {
    let msg_type: &str = msg_type.into();
    let rec = sqlx::query!(
        r#"
        INSERT INTO messages (from_user, to_user, data, type)
        VALUES ($1, $2, $3, $4)
        RETURNING id
        "#,
        from_user,
        to_user,
        data,
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
