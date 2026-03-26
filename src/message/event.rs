use crate::db::{self};
use crate::id::id;
use crate::message::{Ack, MessageType};
use crate::state::group::Permissions;
use crate::state::group::{ChannelType, Group};
use crate::state::user::{self, Voice, VoiceType};
use crate::{State, message::Message};
use anyhow::Result;
use dashmap::Entry;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "event", content = "payload")]
pub enum Event {
    /* ===== USER ===== */
    UpdateUser {
        name: Option<String>,
        avatar: Option<String>,
    },
    ChangeStatus(user::Status),

    /* ===== FRIEND ===== */
    FriendRequest,
    FriendAccept,
    FriendRemove,

    /* ===== Group ===== */
    CreateGroup {
        name: String,
        icon: Option<String>,
        description: Option<String>,
    },
    UpdateGroup {
        name: Option<String>,
        description: Option<String>,
        icon: Option<String>,
    },
    DeleteGroup,
    Subscribe,
    Unsubscribe,

    /* ===== ROLE ===== */
    CreateRole {
        name: String,
        color: String,
        permissions: u64,
    },
    UpdateRole {
        role: id,
        name: Option<String>,
        position: Option<usize>,
        color: Option<String>,
        permissions: Option<u64>,
    },
    DeleteRole {
        role: id,
    },
    AssignRole {
        user: id,
        role: id,
    },
    RemoveRole {
        user: id,
        role: id,
    },

    /* ===== CHANNEL ===== */
    CreateChannel {
        name: String,
        is_voice: bool,
        title: Option<String>,
    },
    UpdateChannel {
        name: Option<String>,
        title: Option<String>,
        position: Option<usize>,
    },
    DeleteChannel,

    /* ===== VOICE ===== */
    JoinVoice,
    ExitVoice,
    MoveToVoice(id),

    /* ===== MODERATION ===== */
    KickUser,
    BanUser,

    // ==== webRTC ====
    Offer(String),
    Answer(String),
    IceCandidate(String),
}

pub async fn handle_event(
    message: Message<Event>,
    connection_id: usize,
    state: &State,
) -> Result<()> {
    let _user_lock = state.user_locks.write(message.from).await;

    match message.r#type {
        MessageType::Info => {
            match message.data {
                /* ===== USER ===== */
                Event::UpdateUser { name, avatar } => {
                    db::user::update_user(&state.pool, message.from, name.clone(), avatar.clone())
                        .await?;

                    if let Some(mut user) = state.users.get_mut(&message.from) {
                        if let Some(name) = name.clone() {
                            user.state.name = name;
                        }
                        user.state.avatar = avatar.clone();

                        let ack = Message {
                            id: message.id,
                            from: message.from,
                            data: Ack::UpdatedUser { name, avatar },
                            ..Message::default()
                        };

                        let user = user.downgrade();

                        user.send_message_all(ack, &state);
                    }
                }

                Event::ChangeStatus(change_status) => {
                    if let Some(mut user) = state.users.get_mut(&message.from) {
                        user.state.status = change_status.clone();

                        let ack = Message {
                            id: message.id,
                            from: message.from,
                            data: Ack::ChangedStatus(change_status),
                            ..Message::default()
                        };

                        let user = user.downgrade();

                        user.send_message_all(ack, &state);
                    }
                }

                /* ===== FRIEND ===== */
                Event::FriendRequest => {
                    let from = message.from;
                    let to = message.to;

                    if from == to {
                        anyhow::bail!("Cannot send friend request to yourself");
                    }

                    let _user2_lock = state.user_locks.write(to).await;

                    db::user::friend_request(&state.pool, from, to).await?;

                    if let Some(mut to_user) = state.users.get_mut(&to) {
                        let ack = Message {
                            id: message.id,
                            from,
                            to,
                            data: Ack::ReceivedFriendRequest,
                            ..Message::default()
                        };

                        to_user.state.friend_requests.push(from);
                        to_user.send_message(ack);
                    }

                    if let Some(mut from_user) = state.users.get_mut(&from) {
                        let ack = Message {
                            id: message.id,
                            from: to,
                            to: from,
                            data: Ack::SentFriendRequest,
                            ..Message::default()
                        };

                        from_user.state.friend_requests_sent.push(to);
                        from_user.send_message(ack);
                    }
                }

                Event::FriendAccept => {
                    let from = message.from;
                    let to = message.to;

                    if from == to {
                        anyhow::bail!("Cannot accept friend request from yourself");
                    }

                    let _user2_lock = state.user_locks.write(to).await;

                    if db::user::friend_accept(&state.pool, from, to).await? {
                        if let Some(mut from_user) = state.users.get_mut(&from) {
                            let ack = Message {
                                id: message.id,
                                from: to,
                                to: from,
                                data: Ack::AddedFriend,
                                ..Message::default()
                            };

                            from_user.state.friends.insert(to);
                            from_user.state.friend_requests.retain(|&i| i != to);
                            from_user.send_message(ack);
                        }

                        if let Some(mut to_user) = state.users.get_mut(&to) {
                            let ack = Message {
                                id: message.id,
                                from,
                                to,
                                data: Ack::AddedFriend,
                                ..Message::default()
                            };

                            to_user.state.friends.insert(from);
                            to_user.state.friend_requests_sent.retain(|&i| i != from);
                            to_user.send_message(ack);
                        }
                    }
                }

                Event::FriendRemove => {
                    let from = message.from;
                    let to = message.to;

                    if from == to {
                        anyhow::bail!("Cannot remove friend from yourself");
                    }

                    let _user2_lock = state.user_locks.write(to).await;

                    db::user::friend_remove(&state.pool, from, to).await?;

                    if let Some(mut from_user) = state.users.get_mut(&from) {
                        let ack = Message {
                            id: message.id,
                            from: to,
                            to: from,
                            data: Ack::DeletedFriend,
                            ..Message::default()
                        };

                        from_user.state.friends.remove(&to);
                        from_user.state.friend_requests_sent.retain(|&i| i != to);
                        from_user.send_message(ack);
                    }

                    if let Some(mut to_user) = state.users.get_mut(&to) {
                        let ack = Message {
                            id: message.id,
                            from,
                            to,
                            data: Ack::DeletedFriend,
                            ..Message::default()
                        };

                        to_user.state.friends.remove(&from);
                        to_user.state.friend_requests.retain(|&i| i != from);
                        to_user.send_message(ack);
                    }
                }

                /* ===== GROUP ===== */
                Event::CreateGroup {
                    name,
                    icon,
                    description,
                } => {
                    let group_id = db::group::create_group(
                        &state.pool,
                        &name,
                        icon.as_deref(),
                        description.as_deref(),
                        message.from,
                    )
                    .await?;

                    db::group::add_member(&state.pool, message.from, group_id).await?;

                    db::group::create_channel(
                        &state.pool,
                        group_id,
                        "general".to_string(),
                        None,
                        false,
                    )
                    .await?;

                    db::group::create_channel(
                        &state.pool,
                        group_id,
                        "general".to_string(),
                        None,
                        true,
                    )
                    .await?;

                    let ack = Message {
                        id: message.id,
                        from: group_id,
                        to: message.from,
                        data: Ack::CreatedGroup {
                            name,
                            icon,
                            description,
                        },
                        ..Message::default()
                    };

                    if let Some(mut user) = state.users.get_mut(&message.from) {
                        user.state.groups.push(group_id);
                        user.next_version();
                        user.send_message(ack);
                    }
                }

                Event::Offer(_) | Event::Answer(_) | Event::IceCandidate(_) => {
                    if let Some(user) = state.users.get(&message.to) {
                        user.send_message(message);
                    }
                }

                _ => anyhow::bail!("Invalid event"),
            }
        }

        MessageType::InfoGroup(group_id) => {
            match message.data {
                /* ===== Group ===== */
                Event::Subscribe => {
                    if !state.groups.contains_key(&group_id) {
                        let _write_guard = state.group_locks.write(group_id).await;
                        if !state.groups.contains_key(&group_id) {
                            let group: Group =
                                db::group::init_group(&state.pool, group_id).await?.into();
                            state.groups.insert(group_id, group);
                        }
                    }

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        if let Some(user) = state.users.get(&message.from) {
                            if !user.state.groups.contains(&group_id) {
                                anyhow::bail!("User is not a member of this group");
                            }

                            group.subscribe(message.from);

                            let ack = Message {
                                id: message.id,
                                from: group_id,
                                data: Ack::Subscribed(Box::new(group.clone())),
                                ..Message::default()
                            };

                            user.send_message(ack);
                        }
                    }
                }

                Event::Unsubscribe => {
                    if let Entry::Occupied(mut group) = state.groups.entry(group_id) {
                        group.get_mut().unsubscribe(message.from);

                        if let Some(user) = state.users.get(&message.from) {
                            let ack = Message {
                                id: message.id,
                                from: group_id,
                                to: message.from,
                                data: Ack::Unsubscribed,
                                ..Message::default()
                            };

                            user.send_message(ack);
                        }

                        if group.get().subscribers.is_empty() {
                            group.remove();
                        }
                    } else {
                        anyhow::bail!("Group not found");
                    }
                }

                Event::UpdateGroup {
                    name,
                    description,
                    icon,
                } => {
                    if let Some(group) = state.groups.get(&group_id) {
                        let perms = group.compute_permissions(message.from, None);
                        if !perms.contains(Permissions::MANAGE_GROUP | Permissions::ADMINISTRATOR) {
                            anyhow::bail!("Unauthorized to update group");
                        }
                    } else {
                        anyhow::bail!("Group not found");
                    }

                    let _lock = state.group_locks.write(group_id).await;

                    db::group::update_group(
                        &state.pool,
                        group_id,
                        name.clone(),
                        description.clone(),
                        icon.clone(),
                    )
                    .await?;

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        group.update_group(name.clone(), icon.clone());

                        let ack = Message {
                            id: message.id,
                            from: group_id,
                            to: group_id,
                            data: Ack::UpdatedGroup {
                                name,
                                description,
                                icon,
                            },
                            ..Message::default()
                        };

                        group.notify_all(ack, &state);
                    }
                }

                /* ===== CHANNEL ===== */
                Event::CreateChannel {
                    name,
                    is_voice,
                    title,
                } => {
                    if let Some(group) = state.groups.get(&group_id) {
                        let perms = group.compute_permissions(message.from, None);
                        if !perms
                            .contains(Permissions::MANAGE_CHANNELS | Permissions::ADMINISTRATOR)
                        {
                            anyhow::bail!("Unauthorized to create channel");
                        }
                    } else {
                        anyhow::bail!("Group not found");
                    }

                    let _lock = state.group_locks.write(group_id).await;

                    let channel_id = db::group::create_channel(
                        &state.pool,
                        group_id,
                        name.clone(),
                        title.clone(),
                        is_voice,
                    )
                    .await?;

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        let position =
                            group.create_channel(channel_id, name.clone(), is_voice, title.clone());

                        let ack = Message {
                            id: message.id,
                            from: group_id,
                            to: channel_id,
                            data: Ack::CreatedChannel {
                                name,
                                position,
                                is_voice,
                                title,
                            },
                            ..Message::default()
                        };

                        group.notify(ack, &state);
                    }
                }

                Event::UpdateChannel {
                    name,
                    title,
                    position,
                } => {
                    let channel_id = message.to;

                    if let Some(group) = state.groups.get(&group_id) {
                        let perms = group.compute_permissions(message.from, Some(channel_id));
                        if !perms
                            .contains(Permissions::MANAGE_CHANNELS | Permissions::ADMINISTRATOR)
                        {
                            anyhow::bail!("Unauthorized to create channel");
                        }
                    } else {
                        anyhow::bail!("Group not found");
                    }

                    let _lock = state.group_locks.write(group_id).await;

                    db::group::update_channel(
                        &state.pool,
                        group_id,
                        channel_id,
                        name.clone(),
                        title.clone(),
                        position,
                    )
                    .await?;

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        group.update_channel(channel_id, name.clone(), title.clone(), position);

                        let ack = Message {
                            id: message.id,
                            from: group_id,
                            to: channel_id,
                            data: Ack::UpdatedChannel {
                                name,
                                title,
                                position,
                            },
                            ..Message::default()
                        };

                        group.notify(ack, &state);
                    }
                }

                Event::JoinVoice => {
                    let channel_id = message.to;

                    if let Some(group) = state.groups.get(&group_id) {
                        let perms = group.compute_permissions(message.from, Some(channel_id));
                        /*if !perms.contains(Permissions::CONNECT) {
                            anyhow::bail!("Unauthorized to join channel");
                        }*/
                    } else {
                        anyhow::bail!("Group not found");
                    }

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        let Some(channel) = group.channels.get_mut(&channel_id) else {
                            anyhow::bail!("Channel not found");
                        };

                        match channel.r#type {
                            ChannelType::Voice(ref mut users) => {
                                users.insert(message.from);
                            }
                            ChannelType::Text => {
                                anyhow::bail!("Cannot join a text channel");
                            }
                        }

                        let group = group.downgrade();

                        if let Some(mut user) = state.users.get_mut(&message.from) {
                            if user.state.voice.is_some() {
                                anyhow::bail!("User is already in a voice channel");
                            }

                            user.state.voice = Some(Voice {
                                connection_id,
                                r#type: VoiceType::Channel {
                                    group_id,
                                    channel_id,
                                },
                            });
                        }

                        let ack = Message {
                            id: message.id,
                            from: group_id,
                            to: message.from,
                            data: Ack::JoinedVoice(channel_id),
                            ..Message::default()
                        };

                        group.notify(ack, &state);
                    }
                }

                Event::ExitVoice => {
                    let channel_id = message.to;

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        let Some(channel) = group.channels.get_mut(&channel_id) else {
                            anyhow::bail!("Channel not found");
                        };

                        match channel.r#type {
                            ChannelType::Voice(ref mut users) => {
                                users.remove(&message.from);
                            }
                            ChannelType::Text => {
                                anyhow::bail!("Cannot join a text channel");
                            }
                        }

                        let group = group.downgrade();

                        if let Some(mut user) = state.users.get_mut(&message.from) {
                            user.state.voice = None;
                        }

                        let ack = Message {
                            id: message.id,
                            from: group_id,
                            to: message.from,
                            data: Ack::ExitedVoice(channel_id),
                            ..Message::default()
                        };

                        group.notify(ack, &state);
                    }
                }

                Event::MoveToVoice(channel_id) => {
                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        let Some(channel) = group.channels.get_mut(&channel_id) else {
                            anyhow::bail!("Channel not found");
                        };

                        match channel.r#type {
                            ChannelType::Voice(ref mut users) => {
                                users.insert(message.from);
                            }
                            ChannelType::Text => {
                                anyhow::bail!("Cannot join a text channel");
                            }
                        }

                        let ack = Message {
                            id: message.id,
                            from: group_id,
                            to: message.from,
                            data: Ack::MovedToVoice(channel_id),
                            ..Message::default()
                        };

                        group.notify(ack, &state);
                    }
                }

                /* ===== MEMBER ===== */

                /* ===== ROLE ===== */
                Event::CreateRole {
                    name,
                    permissions,
                    color,
                } => {
                    if let Some(group) = state.groups.get(&group_id) {
                        let perms = group.compute_permissions(message.from, None);
                        if !perms.contains(Permissions::MANAGE_ROLES | Permissions::ADMINISTRATOR) {
                            anyhow::bail!("Unauthorized to create role");
                        }
                    } else {
                        anyhow::bail!("Group not found");
                    }

                    let _lock = state.group_locks.write(group_id).await;

                    let role_id = db::group::create_role(
                        &state.pool,
                        group_id,
                        name.clone(),
                        color.clone(),
                        permissions,
                    )
                    .await?;

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        group.create_role(
                            role_id,
                            name.clone(),
                            color.clone(),
                            Permissions::from_bits_truncate(permissions),
                        );

                        let ack = Message {
                            id: message.id,
                            from: group_id,
                            to: role_id,
                            data: Ack::CreatedRole {
                                name,
                                color,
                                permissions,
                            },
                            ..Message::default()
                        };

                        group.notify(ack, &state);
                    }
                }
                Event::UpdateRole {
                    role,
                    name,
                    position,
                    color,
                    permissions,
                } => {
                    if let Some(group) = state.groups.get(&group_id) {
                        let perms = group.compute_permissions(message.from, None);
                        if !perms.contains(Permissions::MANAGE_ROLES | Permissions::ADMINISTRATOR) {
                            anyhow::bail!("Unauthorized to update role");
                        }
                    } else {
                        anyhow::bail!("Group not found");
                    }

                    let _lock = state.group_locks.write(group_id).await;

                    db::group::update_role(
                        &state.pool,
                        group_id,
                        role,
                        name.clone(),
                        color.clone(),
                        permissions,
                        position,
                    )
                    .await?;

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        group.update_role(
                            role,
                            name.clone(),
                            position,
                            color.clone(),
                            permissions.map(|p| Permissions::from_bits_truncate(p)),
                        );

                        let ack = Message {
                            id: message.id,
                            from: group_id,
                            to: role,
                            data: Ack::UpdatedRole {
                                name,
                                color,
                                permissions,
                            },
                            ..Message::default()
                        };

                        group.notify(ack, &state);
                    }
                }

                Event::DeleteRole { role } => {
                    if let Some(group) = state.groups.get(&group_id) {
                        let perms = group.compute_permissions(message.from, None);
                        if !perms.contains(Permissions::MANAGE_ROLES | Permissions::ADMINISTRATOR) {
                            anyhow::bail!("Unauthorized to delete role");
                        }
                    } else {
                        anyhow::bail!("Group not found");
                    }

                    let _lock = state.group_locks.write(group_id).await;

                    db::group::delete_role(&state.pool, role).await?;

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        group.delete_role(role);

                        let ack = Message {
                            id: message.id,
                            from: group_id,
                            to: role,
                            data: Ack::DeletedRole,
                            ..Message::default()
                        };

                        group.notify(ack, &state);
                    }
                }

                Event::AssignRole { user, role } => {
                    if let Some(group) = state.groups.get(&group_id) {
                        let perms = group.compute_permissions(message.from, None);
                        if !perms.contains(Permissions::MANAGE_ROLES | Permissions::ADMINISTRATOR) {
                            anyhow::bail!("Unauthorized to assign role");
                        }
                    } else {
                        anyhow::bail!("Group not found");
                    }

                    let _lock = state.group_locks.write(group_id).await;

                    db::group::assign_role(&state.pool, user, role, group_id).await?;

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        group.assign_role(user, role);

                        let ack = Message {
                            id: message.id,
                            from: group_id,
                            to: user,
                            data: Ack::AssignedRole { role_id: role },
                            ..Message::default()
                        };

                        group.notify(ack, &state);
                    }
                }
                _ => anyhow::bail!("Invalid event"),
            }
        }
        _ => {
            anyhow::bail!("Invalid message type");
        }
    }
    Ok(())
}
