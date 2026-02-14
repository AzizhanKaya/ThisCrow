use super::ack::Ack;
use super::event;
use crate::id::id;
use crate::message::{Event, Message, MessageType};
use crate::state::group::Permissions;
use crate::{State, msgpack};
use anyhow::Result;
use bytes::Bytes;
use chrono::Utc;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Text {
    text: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct MultiData {
    images: Option<Vec<String>>,
    videos: Option<Vec<String>>,
    files: Option<Vec<String>>,
    text: Option<String>,
}

pub async fn handle_bytes(user_id: id, state: &State, bytes: Bytes) -> Result<()> {
    if let Ok(mut message) = rmp_serde::from_slice::<Message<Text>>(&bytes) {
        let message_id = message.id;
        let now = Utc::now();
        message.from = user_id;
        message.time = now;
        message.id = id::from(state.snowflake.generate().await);

        let ack = Message {
            id: message.id,
            data: Ack::Received(message_id),
            time: now,
            to: user_id,
            ..Default::default()
        };

        send_message(&state, ack);

        dispatch_message(state, message);

        return Ok(());
    }

    if let Ok(mut event) = rmp_serde::from_slice::<Message<Event>>(&bytes) {
        event.id = id(0);
        event.from = user_id;
        event.time = Utc::now();

        event::handle_event(state, event).await?;
        return Ok(());
    }

    if let Ok(mut message) = rmp_serde::from_slice::<Message<MultiData>>(&bytes) {
        message.from = user_id;
        let message_id = message.id;
        let now = Utc::now();
        message.time = now;
        message.id = id::from(state.snowflake.generate().await);

        let ack = Message {
            id: message.id,
            data: Ack::Received(message_id),
            time: now,
            to: user_id,
            ..Default::default()
        };

        send_message(&state, ack);

        dispatch_message(state, message);

        return Ok(());
    }

    anyhow::bail!("Unkown message struct: {:?}", bytes);
}

fn dispatch_message<T: Serialize + Clone>(state: &State, message: Message<T>) {
    if matches!(message.r#type, MessageType::Direct | MessageType::Group(_)) {
        state.messages.write(message.clone().into());
    }

    match message.r#type {
        MessageType::Direct | MessageType::Info => {
            send_message(state, message.clone());
        }
        MessageType::Group(group_id) | MessageType::InfoGroup(group_id) => {
            send_group_message(state, message, group_id);
        }
        _ => {}
    }
}

pub fn send_message<T: Serialize>(state: &State, message: Message<T>) {
    if let Some(to) = state.users.get(&message.to) {
        to.tx.send(Bytes::from(msgpack!(message)));
    }
}

pub fn send_message_all<T: Serialize>(state: &State, message: Message<T>, user_ids: Vec<id>) {
    let message = Bytes::from(msgpack!(message));

    user_ids
        .into_iter()
        .filter_map(|user_id| state.users.get(&user_id))
        .for_each(|user| {
            user.tx.send(message.clone());
        });
}

fn send_group_message<T: Serialize>(state: &State, message: Message<T>, group_id: id) {
    let Some(group) = state.groups.get(&group_id) else {
        return;
    };

    let user_ids: Vec<id> = group
        .subscribers
        .iter()
        .filter(|&&user_id| {
            let perms = group.compute_permissions(user_id, Some(message.to));

            perms.contains(Permissions::VIEW_MESSAGES)
        })
        .cloned()
        .collect();

    send_message_all(state, message, user_ids);
}
