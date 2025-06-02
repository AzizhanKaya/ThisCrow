use std::collections::HashSet;

use crate::{DashMap, Deserialize, PgPool, Serialize, Utc};

pub type id = uuid::Uuid;

#[derive(sqlx::FromRow, Serialize, Deserialize, Clone)]
pub struct Group {
    pub id: id,
    pub name: String,
    pub users: Vec<id>,
    pub admin: Vec<id>,
    pub description: Option<String>,
    pub created_by: id,
    pub created_at: chrono::DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum State {
    Online,
    Idle,
    Dnd, // Do Not Disturb
    Ghost,
    Offline,
}

#[derive(Clone)]
pub struct User {
    pub username: String,
    pub state: State,
    pub ws_conn: actix_ws::Session, // Web socket connection
}

pub struct AppState {
    pub users: DashMap<id, User>,
    pub chats: DashMap<id, VoiceChat>,
    pub pool: PgPool,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum Events {
    JoinChannel { id: id, direct: bool },
    ExitChannel,
    ChangeState(State),
    FriendReq(id),
    JoinReq(id),
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Event {
    pub time: chrono::DateTime<Utc>,
    pub event: Events,
}

#[derive(Serialize, Deserialize, Clone, sqlx::Type, Default)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "text")]
#[sqlx(rename_all = "lowercase")]
pub enum MessageType {
    Direct,
    Group,
    #[default]
    Server,
    Info,
}

impl From<MessageType> for &'static str {
    fn from(message_type: MessageType) -> Self {
        match message_type {
            MessageType::Direct => "direct",
            MessageType::Group => "group",
            MessageType::Server => "server",
            MessageType::Info => "info",
        }
    }
}

impl From<String> for MessageType {
    fn from(value: String) -> Self {
        match value.as_str() {
            "direct" => MessageType::Direct,
            "group" => MessageType::Group,
            "server" => MessageType::Server,
            "info" => MessageType::Info,
            _ => panic!("Unkown Type"),
        }
    }
}

#[derive(sqlx::FromRow, Serialize, Deserialize, Clone, Default)]
pub struct Message {
    #[serde(skip_deserializing, default)]
    pub id: id,
    #[serde(skip_deserializing, default)]
    pub from: id,
    #[serde(skip_serializing)]
    pub to: id,
    pub data: serde_json::Value,
    pub time: chrono::DateTime<Utc>,
    pub r#type: MessageType,
}

pub struct VoiceChat {
    pub id: id,
    pub users: HashSet<id>,
}
