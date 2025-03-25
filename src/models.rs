use crate::{Serialize, Deserialize, Utc, PgPool, HashMap, DashMap};

#[derive(sqlx::FromRow, Serialize, Deserialize, Clone)]
pub struct UserDB {
    pub id: uuid::Uuid,
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub created_at: chrono::DateTime<Utc>,
}

#[derive(sqlx::FromRow, Serialize, Deserialize, Clone)]
pub struct Group {
    pub id: uuid::Uuid,
    pub name: String,
    pub users: Vec<uuid::Uuid>,
    pub admin: Vec<uuid::Uuid>,
    pub description: Option<String>,
    pub created_by: uuid::Uuid,
    pub created_at: chrono::DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum State {
    Online,
    Idle,
    Dnd, // Do Not Disturb
    Ghost
}

struct webRTC;

pub struct User {
    pub username: String,
    pub state: State,
    pub ws_conn: actix_ws::Session, // Web socket connection
    pub rtc_conn: Option<webRTC> // Voice-chat
}

pub struct AppState {
    users: DashMap<uuid::Uuid, User>,
    pool: PgPool
} 


#[derive(Serialize, Deserialize, Clone)]
pub enum MessageType {
    Direct,
    Group,
    Info
}

#[derive(sqlx::FromRow, Serialize, Deserialize, Clone)]
pub struct Message {
    pub id: uuid::Uuid,
    pub to: uuid::Uuid,
    pub data: String,
    pub time: chrono::DateTime<Utc>,
    pub message_type: MessageType
}


pub struct VoiceChat {
    id: uuid::Uuid,
    name: String,
    users: Vec<uuid::Uuid>,
}