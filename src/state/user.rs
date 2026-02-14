use crate::{id::id, message::Message, msgpack};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;
use tokio::sync::mpsc::UnboundedSender;

pub struct Session {
    pub id: id,
    pub state: State,
    pub tx: UnboundedSender<Bytes>,
}

#[derive(Clone, Serialize)]
pub struct State {
    pub id: id,
    pub version: id,
    pub username: String,
    pub name: String,
    pub avatar: Option<String>,
    pub status: Status,
    pub friends: HashSet<id>,
    pub friend_requests: Vec<id>,
    pub friend_requests_sent: Vec<id>,
    pub groups: Vec<id>,
    pub dms: Vec<id>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub enum Status {
    #[default]
    Online,
    Idle,
    Dnd,
    Offline,
}

impl Session {
    pub fn next_version(&mut self) -> id {
        self.state.version.add(1)
    }

    pub fn get_version(&self) -> id {
        self.state.version
    }

    pub fn send_bytes(&self, message: Bytes) {
        self.tx.send(message);
    }

    pub fn send_message<T: Serialize>(&self, message: Message<T>) {
        self.tx.send(Bytes::from(msgpack!(message)));
    }
}
