use super::ack::Ack;
use super::event;
use crate::TOKIO_RT;
use crate::db::message::StoredMessage;
use crate::id::id;
use crate::message::{Event, Message, MessageType};
use crate::state::group::Permissions;
use crate::{State, msgpack};
use anyhow::Result;
use bytes::Bytes;
use rmpv::Value;
use serde::{Deserialize, Deserializer, Serialize, de};

#[derive(Debug, Clone, Serialize)]
#[serde_with::skip_serializing_none]
pub struct MultiData {
    text: Option<String>,
    images: Option<Vec<String>>,
    videos: Option<Vec<String>>,
    files: Option<Vec<String>>,
}

impl<'de> Deserialize<'de> for MultiData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize, Debug)]
        struct Helper {
            text: Option<String>,
            images: Option<Vec<String>>,
            videos: Option<Vec<String>>,
            files: Option<Vec<String>>,
        }

        let helper = Helper::deserialize(deserializer)?;

        if helper.text.is_none()
            && helper.images.is_none()
            && helper.videos.is_none()
            && helper.files.is_none()
        {
            return Err(de::Error::custom("At least one field must be Some"));
        }
        Ok(MultiData {
            text: helper.text,
            images: helper.images,
            videos: helper.videos,
            files: helper.files,
        })
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum Data {
    Text(String),
    Encrypted {
        #[serde(with = "serde_bytes")]
        nonce: Vec<u8>,
        #[serde(with = "serde_bytes")]
        cipher: Vec<u8>,
    },
    MultiData(MultiData),
}

pub async fn handle_bytes(
    bytes: Bytes,
    user_id: id,
    connection_id: usize,
    state: &State,
) -> Result<()> {
    if let Ok(mut message) = rmp_serde::from_slice::<Message<Data>>(&bytes) {
        let message_id = message.id;
        message.from = user_id;
        message.id = state.snowflake.generate();

        let ack = Message {
            id: message.id,
            data: Ack::Received(message_id),
            from: match message.r#type {
                MessageType::Group(gid) | MessageType::InfoGroup(gid) => gid,
                MessageType::Direct | MessageType::Info | MessageType::Server => id(0),
            },
            to: message.to,
            ..Default::default()
        };

        if let Some(mut user) = state.users.get_mut(&user_id) {
            user.state.dms.insert(message.to);
            user.next_version();

            let message = Bytes::from(msgpack!(message));

            for connection in user.connections.iter() {
                if connection.id == connection_id {
                    connection.writer.send(Bytes::from(msgpack!(ack)));
                    continue;
                }
                connection.writer.send(message.clone());
            }
        }

        dispatch_message(state, message);

        return Ok(());
    }

    if let Ok(mut event) = rmp_serde::from_slice::<Message<Event>>(&bytes) {
        event.from = user_id;

        if *user_id != (*event.id >> 32) as i32 {
            anyhow::bail!("Invalid event id");
        }

        let state = state.clone();

        TOKIO_RT.spawn(async move {
            let event_id = event.id;

            if let Err(e) = event::handle_event(event, connection_id, &state).await {
                log::warn!("Error while handling event: {:?}", e);

                if let Some(user) = state.users.get(&user_id) {
                    user.send_message(Message {
                        id: event_id,
                        from: user_id,
                        data: Ack::Error(e.to_string()),
                        ..Default::default()
                    });
                }
            }
        });

        return Ok(());
    }

    anyhow::bail!(
        "Unkown message struct: {:?}",
        rmp_serde::from_slice::<Message<Value>>(&bytes)
    );
}

fn dispatch_message<T: Serialize + Clone>(state: &State, message: Message<T>) -> Result<()> {
    if matches!(message.r#type, MessageType::Direct | MessageType::Group(_)) {
        let message: StoredMessage = message.clone().try_into()?;
        state.messages.save_message(message)?;
    }

    match message.r#type {
        MessageType::Direct | MessageType::Info => {
            send_message(state, message);
        }
        MessageType::Group(group_id) | MessageType::InfoGroup(group_id) => {
            send_group_message(state, message, group_id);
        }
        _ => {}
    }

    Ok(())
}

pub fn send_message<T: Serialize>(state: &State, message: Message<T>) {
    if let Some(mut user) = state.users.get_mut(&message.to) {
        if matches!(message.r#type, MessageType::Direct) {
            user.state.dms.insert(message.from);
            user.next_version();
        }

        let message = Bytes::from(msgpack!(message));
        for connection in user.connections.iter() {
            connection.writer.send(message.clone());
        }
    }
}

pub fn send_message_all<T: Serialize>(
    state: &State,
    message: Message<T>,
    user_ids: impl Iterator<Item = id>,
) {
    let message = Bytes::from(msgpack!(message));

    user_ids
        .filter_map(|user_id| state.users.get(&user_id))
        .for_each(|user| {
            for connection in user.connections.iter() {
                connection.writer.send(message.clone());
            }
        });
}

fn send_group_message<T: Serialize>(state: &State, message: Message<T>, group_id: id) {
    let Some(group) = state.groups.get(&group_id) else {
        return;
    };

    let channel_id = message.to;

    let user_ids = group
        .subscribers
        .iter()
        .filter(|&&user_id| {
            let perms = group.compute_permissions(user_id, Some(channel_id));

            perms.contains(Permissions::VIEW_MESSAGES)
        })
        .copied();

    send_message_all(state, message, user_ids);
}
