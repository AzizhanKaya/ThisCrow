use crate::{
    id::id,
    message::{Event, Message, dispatch},
    msgpack,
};
use bytes::Bytes;
use flume::Sender;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub struct Session {
    pub state: State,
    pub connections: Vec<Connection>,
    pub event_tx: Sender<Message<Event>>,
}

pub struct Connection {
    pub id: usize,
    pub writer: Sender<Bytes>,
}

#[derive(Clone, Serialize, Debug, Default)]
pub struct State {
    pub id: id,
    pub username: String,
    pub name: String,
    pub avatar: Option<String>,
    pub banner: Option<String>,
    pub status: Status,
    pub friends: HashSet<id>,
    pub friend_requests: Vec<id>,
    pub friend_requests_sent: Vec<id>,
    pub dms: HashSet<id>,
    pub groups: Vec<id>,
    pub activities: Vec<Activity>,
    pub voice: Option<Voice>,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub enum Status {
    Online,
    Idle,
    Dnd,
    #[default]
    Offline,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum Activity {
    Game(Game),
    Music(Music),
    Watching {
        video: id,
        offset: f64,
        playing: bool,
    },
    Streaming {
        group_id: id,
        channel_id: id,
        time: f64,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Music {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_url: String,
    pub length: u64,
    pub offset: f64,
    pub paused: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Game {
    pub app_id: u64,
    pub start_time: f64,
    pub name: String,
    pub header_image: String,
    pub short_description: String,
    pub background: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Voice {
    pub connection_id: usize,
    pub r#type: VoiceType,
}

#[derive(Clone, Debug, Serialize)]
pub enum VoiceType {
    Direct(id),
    Channel { group_id: id, channel_id: id },
}

impl Session {
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

        let all_users: HashSet<id> = self
            .state
            .friends
            .iter()
            .chain(groups.iter().flat_map(|group| group.subscribers.iter()))
            .chain(self.state.friend_requests.iter())
            .chain(self.state.friend_requests_sent.iter())
            .chain(self.state.dms.iter())
            .chain(std::iter::once(&self.state.id))
            .copied()
            .collect();

        dispatch::send_message_all(&state, message, all_users.into_iter());
    }
}
