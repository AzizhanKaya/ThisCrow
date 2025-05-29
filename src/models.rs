use crate::{Arc, DashMap, Deserialize, PgPool, Serialize, Utc};

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
}

pub struct webRTC {
    pub peer_connection: webrtc::peer_connection::RTCPeerConnection,
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
    pub rtc_api: Arc<webrtc::api::API>,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum EventType {
    JoinChannel(id),
    ExitChannel,
    ChangeState(State),
    FriendReq(id),
    JoinReq(id),
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Event {
    pub time: chrono::DateTime<Utc>,
    pub r#type: EventType,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum MessageType {
    Direct,
    Group,
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

#[derive(sqlx::FromRow, Serialize, Deserialize, Clone)]
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
    pub users: DashMap<id, webRTC>,
}
