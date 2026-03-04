use crate::{
    id::id,
    message::{Message, dispatch},
    msgpack,
};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tokio::sync::mpsc::UnboundedSender;

pub struct Session {
    pub state: State,
    pub connections: Vec<Connection>,
}

pub struct Connection {
    pub id: usize,
    pub writer: UnboundedSender<Bytes>,
}

#[derive(Clone, Serialize, Debug)]
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
    pub dms: Vec<id>,
    pub groups: Vec<id>,
    #[serde(skip)]
    pub activities: Vec<Activity>,
    #[serde(skip)]
    pub voice: Option<Voice>,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub enum Status {
    #[default]
    Online,
    Idle,
    Dnd,
    Offline,
}

#[derive(Clone, Debug)]
pub enum Activity {
    Game { name: String, time: DateTime<Utc> },
    Music { name: String, time: DateTime<Utc> },
}

#[derive(Clone, Debug)]
pub enum Voice {
    Direct(id),
    Channel(id),
}

impl Session {
    pub fn next_version(&mut self) -> id {
        self.state.version.add(1)
    }

    pub fn get_version(&self) -> id {
        self.state.version
    }

    pub fn send_bytes(&self, bytes: impl Into<Bytes>) {
        let bytes = bytes.into();
        for connection in self.connections.iter() {
            connection.writer.send(bytes.clone());
        }
    }

    pub fn send_message<T: Serialize>(&self, message: Message<T>) {
        self.send_bytes(msgpack!(message));
    }

    pub fn send_message_all<T: Serialize + Clone>(
        &self,
        message: Message<T>,
        state: &crate::State,
    ) {
        let groups = self
            .state
            .groups
            .iter()
            .filter_map(|group_id| state.groups.get(group_id))
            .collect::<Vec<_>>();

        let mut all_users: HashSet<id> = self
            .state
            .friends
            .iter()
            .chain(groups.iter().flat_map(|group| group.subscribers.iter()))
            .chain(self.state.friend_requests.iter())
            .chain(self.state.friend_requests_sent.iter())
            .chain(self.state.dms.iter())
            .copied()
            .collect();

        all_users.remove(&message.from);

        self.send_message(message.clone());

        dispatch::send_message_all(&state, message, all_users.into_iter().collect());
    }
}
