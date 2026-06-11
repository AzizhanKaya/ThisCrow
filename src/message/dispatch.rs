use super::ack::Ack;
use crate::db::message::StoredMessage;
use crate::id::id;
use crate::message::{Data, Event, Message, MessageType};
use crate::state::group::Permissions;
use crate::{State, msgpack};
use anyhow::Result;
use bytes::Bytes;
use flume::Sender;
use serde::Serialize;

pub async fn handle_bytes(
    bytes: Bytes,
    user_id: id,
    connection_id: usize,
    event_tx: &Sender<(usize, Message<Event>)>,
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

            let user = user.downgrade();

            let message = Bytes::from(msgpack!(message));

            for connection in user.connections.iter() {
                if connection.id == connection_id {
                    connection.writer.send(Bytes::from(msgpack!(ack)));
                    continue;
                }
                connection.writer.send(message.clone());
            }
        }

        let message_id = message.id;

        if let Err(e) = dispatch_message(state, message, connection_id).await {
            if let Some(user) = state.users.get(&user_id) {
                let error = Message {
                    id: message_id,
                    data: Ack::MessageError(e.to_string()),
                    to: user_id,
                    ..Default::default()
                };
                user.send_message(error);
            }
        }

        return Ok(());
    }

    if let Ok(mut event) = rmp_serde::from_slice::<Message<Event>>(&bytes) {
        event.from = user_id;

        if *user_id != (*event.id >> 32) as i32 {
            anyhow::bail!("Invalid event id");
        }

        event_tx.send((connection_id, event))?;

        return Ok(());
    }

    use rmpv::decode::read_value;

    let mut slice = bytes.as_ref();
    let value = read_value(&mut slice)?;

    anyhow::bail!("Unkown message struct: {:?}", value);
}

async fn dispatch_message(
    state: &State,
    message: Message<Data>,
    connection_id: usize,
) -> Result<()> {
    match message.r#type {
        MessageType::Direct => {
            let stored: StoredMessage = message.clone().try_into()?;

            state.messages.write(stored).await?;
            send_message(state, message);
        }

        MessageType::Group(group_id) => {
            let Some(group) = state.groups.get(&group_id) else {
                anyhow::bail!("Group not found")
            };

            if !group
                .compute_permissions(message.from, Some(message.to))
                .contains(Permissions::SEND_MESSAGE)
            {
                anyhow::bail!("You don't have permission to send messages");
            }

            if matches!(message.data, Data::MultiData(_)) {
                if !group
                    .compute_permissions(message.from, Some(message.to))
                    .contains(Permissions::ATTACH_FILES)
                {
                    anyhow::bail!("You don't have permission to attach files");
                }
            }

            let stored: StoredMessage = message.clone().try_into()?;
            state.messages.write(stored).await?;

            send_group_message(state, message, group_id, connection_id);
        }
        _ => {}
    }

    Ok(())
}

pub fn send_message<T: Serialize>(state: &State, message: Message<T>) {
    if let Some(mut user) = state.users.get_mut(&message.to) {
        if matches!(message.r#type, MessageType::Direct) {
            user.state.dms.insert(message.from);
        }

        let message = Bytes::from(msgpack!(message));
        for connection in user.connections.iter() {
            connection.writer.send(message.clone());
        }
    }
}

fn send_group_message<T: Serialize>(
    state: &State,
    message: Message<T>,
    group_id: id,
    connection_id: usize,
) {
    let Some(group) = state.groups.get(&group_id) else {
        return;
    };

    let from = message.from;
    let channel_id = message.to;

    let bytes = Bytes::from(msgpack!(message));

    group
        .subscribers
        .iter()
        .filter(|&&(user_id, conn_id)| {
            let perms = group.compute_permissions(user_id, Some(channel_id));
            perms.contains(Permissions::VIEW_MESSAGES)
                && !(user_id == from && conn_id == connection_id)
        })
        .for_each(|&(user_id, conn_id)| {
            if let Some(user) = state.users.get(&user_id) {
                user.send_bytes_connection(conn_id, bytes.clone());
            }
        })
}
