use crate::{DashMap, PgPool, Utc};
use anyhow::Result;
use chrono::DateTime;
use chrono::{TimeZone, offset::LocalResult};
use dashmap::DashSet;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type id = Uuid;

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
    Dnd,
    Offline,
}

#[derive(Clone)]
pub struct User {
    pub username: String,
    pub state: State,
    pub ws_conn: actix_ws::Session,
    pub voice_chat: Option<id>,
}

pub struct AppState {
    pub users: DashMap<id, User>,
    pub voice_chats: DashMap<id, DashSet<id>>,
    pub pool: PgPool,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum Event {
    JoinChannel { id: id },
    ExitChannel,
    ChangeState(State),
}

#[derive(Serialize, Deserialize, Clone, sqlx::Type, Default, Debug)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "text")]
#[sqlx(rename_all = "lowercase")]
pub enum MessageType {
    Direct,
    Group,
    #[default]
    Server,
    Info,
    InfoGroup,
}

impl From<MessageType> for &'static str {
    fn from(message_type: MessageType) -> Self {
        match message_type {
            MessageType::Direct => "direct",
            MessageType::Group => "group",
            MessageType::Server => "server",
            MessageType::Info => "info",
            MessageType::InfoGroup => "info_group",
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
            "info_group" => MessageType::InfoGroup,
            _ => panic!("Unkown Type"),
        }
    }
}

#[derive(sqlx::FromRow, Serialize, Deserialize, Clone, Default, Debug)]
pub struct Message<T> {
    #[serde(skip_serializing)]
    pub id: id,
    #[serde(skip_deserializing, default)]
    pub from: id,
    pub to: id,
    pub data: T,
    pub time: chrono::DateTime<Utc>,
    pub r#type: MessageType,
}

const MESSAGE_NAMESPACE: Uuid = uuid::uuid!("6ba7b810-9dad-11d1-80b4-00c04fd430c8");

impl<T: Serialize> Message<T> {
    pub fn compute_id(&self) -> Uuid {
        let value = serde_json::to_value(&self).expect("failed to serialize");

        let data_str = canonical_json::to_string(&value).expect("failed to serialize canonically");

        Uuid::new_v5(&MESSAGE_NAMESPACE, data_str.as_bytes())
    }
}
