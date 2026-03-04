use crate::db::{self};
use crate::id::id;
use crate::message::dispatch::{send_message, send_message_all};
use crate::message::{Ack, MessageType};
use crate::state::group::Permissions;
use crate::state::group::{ChannelType, Group};
use crate::state::user;
use crate::{State, message::Message};
use anyhow::anyhow;
use anyhow::{Result, bail};
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
        r#type: ChannelType,
    },
    UpdateChannel {
        name: Option<String>,
        title: Option<String>,
        position: Option<usize>,
    },
    DeleteChannel,

    /* ===== VOICE ===== */
    JoinChannel,
    ExitChannel,
    MoveVoice {
        to: id,
    },

    /* ===== MODERATION ===== */
    KickUser,
    BanUser,

    // ==== webRTC ====
    Offer(String),
    Answer(String),
    IceCandidate(String),
}

pub async fn handle_event(state: State, message: Message<Event>) -> Result<()> {
    match message.r#type {
        MessageType::Info => {
            match message.data {
                Event::UpdateUser { name, avatar } => {
                    let _user_lock = state.user_locks.write(message.from).await;

                    db::user::update_user(&state.pool, message.from, name.clone(), avatar.clone())
                        .await?;

                    if let Some(mut user) = state.users.get_mut(&message.from) {
                        if let Some(name) = name.clone() {
                            user.state.name = name;
                        }
                        user.state.avatar = avatar.clone();

                        let ack = Message {
                            id: user.next_version(),
                            to: message.from,
                            data: Ack::UpdatedUser { name, avatar },
                            ..Message::default()
                        };

                        user.send_message_all(ack, &state);
                    }
                }

                Event::ChangeStatus(change_status) => {
                    if let Some(mut user) = state.users.get_mut(&message.from) {
                        user.state.status = change_status.clone();

                        let ack = Message {
                            id: user.next_version(),
                            from: message.from,
                            data: Ack::ChangedStatus(change_status),
                            ..Message::default()
                        };

                        user.send_message_all(ack, &state);
                    }
                }
                /* ===== FRIEND ===== */
                Event::FriendRequest => {
                    let from = message.from;
                    let to = message.to;

                    let _user1_lock = state.user_locks.write(from).await;
                    let _user2_lock = state.user_locks.write(to).await;

                    db::user::friend_request(&state.pool, from, to).await?;

                    if let Some(mut to_user) = state.users.get_mut(&to) {
                        let ack = Message {
                            id: to_user.next_version(),
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
                            id: from_user.next_version(),
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

                    let _user1_lock = state.user_locks.write(from).await;
                    let _user2_lock = state.user_locks.write(to).await;

                    if db::user::friend_accept(&state.pool, from, to).await? {
                        if let Some(mut from_user) = state.users.get_mut(&from) {
                            let ack = Message {
                                id: from_user.next_version(),
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
                                id: to_user.next_version(),
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

                    let _user1_lock = state.user_locks.write(from).await;
                    let _user2_lock = state.user_locks.write(to).await;

                    db::user::friend_remove(&state.pool, from, to).await?;

                    if let Some(mut from_user) = state.users.get_mut(&from) {
                        let ack = Message {
                            id: from_user.next_version(),
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
                            id: to_user.next_version(),
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

                    let ack = Message {
                        id: group_id,
                        to: message.from,
                        data: Ack::CreatedGroup {
                            name,
                            icon,
                            description,
                        },
                        ..Message::default()
                    };

                    send_message(&state, ack);

                    return Ok(());
                }
                _ => anyhow::bail!("Invalid event"),
            }
        }

        MessageType::InfoGroup(group_id) => {
            let ack: Message<Ack> = match message.data {
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
                        if let Some(user) = state.users.get_mut(&message.from) {
                            if !user.state.groups.contains(&group_id) {
                                return Ok(());
                            }

                            group.subscribe(message.from);

                            let ack = Message {
                                id: group.get_version(),
                                from: message.from,
                                data: Ack::Subscribed(Box::new(group.clone())),
                                ..Message::default()
                            };

                            user.send_message(ack);
                        }
                    }
                    return Ok(());
                }

                Event::Unsubscribe => {
                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        group.unsubscribe(message.from);

                        if let Some(user) = state.users.get_mut(&message.from) {
                            let ack = Message {
                                id: group.get_version(),
                                from: group_id,
                                to: message.from,
                                data: Ack::Unsubscribed,
                                ..Message::default()
                            };

                            user.send_message(ack);
                        }
                        return Ok(());
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
                            id: group.get_version(),
                            from: group_id,
                            data: Ack::UpdatedGroup {
                                name,
                                description,
                                icon,
                            },
                            ..Message::default()
                        };

                        let users = group.members.keys().copied().collect();

                        send_message_all(&state, ack, users);
                    }

                    return Ok(());
                }

                /* ===== CHANNEL ===== */
                Event::CreateChannel { name, r#type } => {
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

                    let channel_id = message.to;

                    let _lock = state.group_locks.write(group_id).await;

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        let position =
                            group.create_channel(channel_id, name.clone(), r#type.clone());

                        Message {
                            id: group.get_version(),
                            from: group_id,
                            to: channel_id,
                            data: Ack::CreatedChannel {
                                name,
                                position,
                                r#type,
                            },
                            ..Message::default()
                        }
                    } else {
                        return Ok(());
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

                        Message {
                            id: group.get_version(),
                            from: group_id,
                            to: channel_id,
                            data: Ack::UpdatedChannel {
                                name,
                                title,
                                position,
                            },
                            ..Message::default()
                        }
                    } else {
                        return Ok(());
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

                        Message {
                            id: group.get_version(),
                            from: group_id,
                            to: role_id,
                            data: Ack::CreatedRole {
                                name,
                                color,
                                permissions,
                            },
                            ..Message::default()
                        }
                    } else {
                        return Ok(());
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
                        role,
                        name.clone(),
                        color.clone(),
                        permissions,
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

                        Message {
                            id: group.get_version(),
                            from: group_id,
                            to: message.from,
                            data: Ack::UpdatedRole {
                                name,
                                color,
                                permissions,
                            },
                            ..Message::default()
                        }
                    } else {
                        anyhow::bail!("Group not found");
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

                        Message {
                            id: group.get_version(),
                            from: role,
                            to: message.from,
                            data: Ack::DeletedRole,
                            ..Message::default()
                        }
                    } else {
                        anyhow::bail!("Group not found");
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

                        Message {
                            id: group.get_version(),
                            from: role,
                            to: message.from,
                            data: Ack::AssignedRole,
                            ..Message::default()
                        }
                    } else {
                        anyhow::bail!("Group not found");
                    }
                }
                _ => anyhow::bail!("Invalid event"),
            };

            if let Some(group) = state.groups.get(&group_id) {
                group.notify(ack, &state);
            } else {
                anyhow::bail!("Group not found");
            }
        }
        _ => {
            anyhow::bail!("Invalid message type");
        }
    }
    Ok(())
}
