use crate::db::{self};
use crate::id::id;
use crate::message::dispatch::send_message_all;
use crate::message::{Ack, MessageType};
use crate::state::group::Permissions;
use crate::state::group::{ChannelType, Group};
use crate::state::user;
use crate::{State, message::Message};
use anyhow::Result;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Serialize, Deserialize, Clone)]
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
    UpdateGroup {
        name: Option<String>,
        icon: Option<String>,
    },
    Subscribe,
    Unsubscribe,

    /* ===== ROLE ===== */
    CreateRole {
        name: String,
        permissions: u64,
        color: String,
    },
    UpdateRole {
        role: id,
        name: Option<String>,
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
        kind: ChannelType,
    },
    UpdateChannel {
        name: Option<String>,
        order: Option<u64>,
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
}

pub async fn handle_event(state: &State, message: Message<Event>) -> Result<()> {
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

                        let all_users: HashSet<id> = user
                            .state
                            .friends
                            .iter()
                            .cloned()
                            .chain(std::iter::once(message.from))
                            .chain(
                                user.state
                                    .groups
                                    .iter()
                                    .filter_map(|group_id| state.groups.get(group_id))
                                    .flat_map(|group| {
                                        group.subscribers.iter().cloned().collect::<Vec<_>>()
                                    }),
                            )
                            .collect();

                        let ack = Message {
                            id: user.next_version(),
                            to: message.from,
                            data: Ack::UpdatedUser {
                                name: name,
                                avatar: avatar,
                            },
                            ..Message::default()
                        };

                        send_message_all(state, ack, all_users.into_iter().collect());
                    }
                }

                Event::ChangeStatus(change_status) => {
                    if let Some(mut user) = state.users.get_mut(&message.from) {
                        user.state.status = change_status.clone();

                        let all_users: HashSet<id> = user
                            .state
                            .friends
                            .iter()
                            .cloned()
                            .chain(std::iter::once(message.from))
                            .chain(
                                user.state
                                    .groups
                                    .iter()
                                    .filter_map(|group_id| state.groups.get(group_id))
                                    .flat_map(|group| {
                                        group.subscribers.iter().cloned().collect::<Vec<_>>()
                                    }),
                            )
                            .collect();

                        let ack = Message {
                            id: user.next_version(),
                            to: message.from,
                            data: Ack::ChangedStatus(change_status),
                            ..Message::default()
                        };

                        send_message_all(state, ack, all_users.into_iter().collect());
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
                            to,
                            data: Ack::ReceivedFriendRequest(from),
                            ..Message::default()
                        };

                        to_user.state.friend_requests.push(from);
                        to_user.send_message(ack);
                    }

                    if let Some(mut from_user) = state.users.get_mut(&from) {
                        let ack = Message {
                            id: from_user.next_version(),
                            to: from,
                            data: Ack::SentFriendRequest(to),
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
                                to: from,
                                data: Ack::AddedFriend(to),
                                ..Message::default()
                            };

                            from_user.state.friends.insert(to);
                            from_user.send_message(ack);
                        }

                        if let Some(mut to_user) = state.users.get_mut(&to) {
                            let ack = Message {
                                id: to_user.next_version(),
                                to,
                                data: Ack::AddedFriend(from),
                                ..Message::default()
                            };

                            to_user.state.friends.insert(from);
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
                            to: from,
                            data: Ack::DeletedFriend(to),
                            ..Message::default()
                        };

                        from_user.state.friends.remove(&to);
                        from_user.send_message(ack);
                    }

                    if let Some(mut to_user) = state.users.get_mut(&to) {
                        let ack = Message {
                            id: to_user.next_version(),
                            to,
                            data: Ack::DeletedFriend(from),
                            ..Message::default()
                        };

                        to_user.state.friends.remove(&from);
                        to_user.send_message(ack);
                    }
                }
                _ => return Err(anyhow!("Invalid event")),
            }
        }

        MessageType::InfoGroup(group_id) => {
            let ack: Message<Ack> = match message.data {
                /* ===== Group ===== */
                Event::Subscribe => {
                    if !state.groups.contains_key(&group_id) {
                        let _write_guard = state.group_locks.write(group_id).await;

                        if !state.groups.contains_key(&group_id) {
                            let group: Group = db::group::init_group(&state.pool, group_id).await?;
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
                                to: message.from,
                                data: Ack::Subscribed(group.clone()),
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

                        if let Some(mut user) = state.users.get_mut(&message.from) {
                            user.state.groups.retain(|&gid| gid != group_id);

                            let ack = Message {
                                id: group.get_version(),
                                to: message.from,
                                data: Ack::Unsubscribed(group_id),
                                ..Message::default()
                            };

                            user.send_message(ack);
                        }
                        return Ok(());
                    } else {
                        return Err(anyhow!("Group not found"));
                    }
                }

                Event::UpdateGroup { name, icon } => {
                    if let Some(group) = state.groups.get(&group_id) {
                        let perms = group.compute_permissions(message.from, None);
                        if !perms.contains(Permissions::MANAGE_GROUP | Permissions::ADMINISTRATOR) {
                            return Err(anyhow!("Unauthorized to update group"));
                        }
                    } else {
                        return Err(anyhow!("Group not found"));
                    }

                    let _lock = state.group_locks.write(group_id).await;

                    db::group::update_group(&state.pool, group_id, name.clone(), icon.clone())
                        .await?;

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        group.update_group(name.clone(), icon.clone());

                        Message {
                            id: group.get_version(),
                            to: message.from,
                            data: Ack::UpdatedGroup {
                                id: group_id,
                                name,
                                icon,
                            },
                            ..Message::default()
                        }
                    } else {
                        return Ok(());
                    }
                }

                /* ===== ROLE ===== */
                Event::CreateRole {
                    name,
                    permissions,
                    color,
                } => {
                    if let Some(group) = state.groups.get(&group_id) {
                        let perms = group.compute_permissions(message.from, None);
                        if !perms.contains(Permissions::MANAGE_ROLES | Permissions::ADMINISTRATOR) {
                            return Err(anyhow!("Unauthorized to create role"));
                        }
                    } else {
                        return Err(anyhow!("Group not found"));
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
                            to: message.from,
                            data: Ack::CreatedRole {
                                id: role_id,
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
                    color,
                    permissions,
                } => {
                    if let Some(group) = state.groups.get(&group_id) {
                        let perms = group.compute_permissions(message.from, None);
                        if !perms.contains(Permissions::MANAGE_ROLES | Permissions::ADMINISTRATOR) {
                            return Err(anyhow!("Unauthorized to update role"));
                        }
                    } else {
                        return Err(anyhow!("Group not found"));
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
                            color.clone(),
                            permissions.map(|p| Permissions::from_bits_truncate(p)),
                        );

                        Message {
                            id: group.get_version(),
                            to: message.from,
                            data: Ack::UpdatedRole {
                                id: role,
                                name,
                                color,
                                permissions,
                            },
                            ..Message::default()
                        }
                    } else {
                        return Err(anyhow!("Group not found"));
                    }
                }

                Event::DeleteRole { role } => {
                    if let Some(group) = state.groups.get(&group_id) {
                        let perms = group.compute_permissions(message.from, None);
                        if !perms.contains(Permissions::MANAGE_ROLES | Permissions::ADMINISTRATOR) {
                            return Err(anyhow!("Unauthorized to delete role"));
                        }
                    } else {
                        return Err(anyhow!("Group not found"));
                    }

                    let _lock = state.group_locks.write(group_id).await;

                    db::group::delete_role(&state.pool, role).await?;

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        group.delete_role(role);

                        Message {
                            id: group.get_version(),
                            to: message.from,
                            data: Ack::DeletedRole(role),
                            ..Message::default()
                        }
                    } else {
                        return Err(anyhow!("Group not found"));
                    }
                }

                Event::AssignRole { user, role } => {
                    if let Some(group) = state.groups.get(&group_id) {
                        let perms = group.compute_permissions(message.from, None);
                        if !perms.contains(Permissions::MANAGE_ROLES | Permissions::ADMINISTRATOR) {
                            return Err(anyhow!("Unauthorized to assign role"));
                        }
                    } else {
                        return Err(anyhow!("Group not found"));
                    }

                    let _lock = state.group_locks.write(group_id).await;

                    db::group::assign_role(&state.pool, user, role, group_id).await?;

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        group.assign_role(user, role);

                        Message {
                            id: group.get_version(),
                            to: message.from,
                            data: Ack::AssignedRole(role),
                            ..Message::default()
                        }
                    } else {
                        return Err(anyhow!("Group not found"));
                    }
                } /*

                Event::RemoveRole { user, role } => {
                if let Some(group) = state.groups.get(&group_id) {
                let perms = group.compute_permissions(message.from, None);
                if !perms.contains(Permissions::MANAGE_ROLES | Permissions::ADMINISTRATOR) {
                return Err(anyhow!("Unauthorized to remove role"));
                }
                } else {
                return Err(anyhow!("Group not found"));
                }

                let _lock = state.group_locks.write(group_id).await;

                db::group::remove_role(&state.pool, user, role).await?;

                if let Some(mut group) = state.groups.get_mut(&group_id) {
                group.remove_role(user, role);

                Message {
                id: group.get_version(),
                to: message.from,
                data: Ack::RemovedRole { user, role },
                ..Message::default()
                }
                } else {
                return Err(anyhow!("Group not found"));
                }
                }

                /* ===== CHANNEL ===== */
                Event::CreateChannel { name, kind } => {
                    if let Some(group) = state.groups.get(&group_id) {
                        let perms = group.compute_permissions(message.from, None);
                        if !perms.contains(Permissions::MANAGE_CHANNELS | Permissions::ADMINISTRATOR) {
                            return Err(anyhow!("Unauthorized to create channel"));
                        }
                    } else {
                        return Err(anyhow!("Group not found"));
                    }

                    let _lock = state.group_locks.write(group_id).await;

                    let channel_id = db::group::create_channel(&state.pool, group_id, name.clone(), kind).await?;

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        group.create_channel(channel_id, name.clone(), kind);

                        Message {
                            id: group.get_version(),
                            to: message.from,
                            data: Ack::CreatedChannel { id: channel_id, name, kind },
                            ..Message::default()
                        }
                    } else {
                        return Err(anyhow!("Group not found"));
                    }
                }

                Event::UpdateChannel { name, order } => {
                     // Kanal ID'si message.to üzerinden geliyor varsayımıyla:
                    let channel_id = message.to;

                    if let Some(group) = state.groups.get(&group_id) {
                        let perms = group.compute_permissions(message.from, None);
                        if !perms.contains(Permissions::MANAGE_CHANNELS | Permissions::ADMINISTRATOR) {
                            return Err(anyhow!("Unauthorized to update channel"));
                        }
                    } else {
                        return Err(anyhow!("Group not found"));
                    }

                    let _lock = state.group_locks.write(group_id).await;

                    db::group::update_channel(&state.pool, channel_id, name.clone(), order).await?;

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        group.update_channel(channel_id, name.clone(), order);

                        Message {
                            id: group.get_version(),
                            to: message.from,
                            data: Ack::UpdatedChannel { id: channel_id, name, order },
                            ..Message::default()
                        }
                    } else {
                        return Err(anyhow!("Group not found"));
                    }
                }

                Event::DeleteChannel => {
                    let channel_id = message.to;

                    if let Some(group) = state.groups.get(&group_id) {
                        let perms = group.compute_permissions(message.from, None);
                        if !perms.contains(Permissions::MANAGE_CHANNELS | Permissions::ADMINISTRATOR) {
                            return Err(anyhow!("Unauthorized to delete channel"));
                        }
                    } else {
                        return Err(anyhow!("Group not found"));
                    }

                    let _lock = state.group_locks.write(group_id).await;

                    db::group::delete_channel(&state.pool, channel_id).await?;

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        group.delete_channel(channel_id);

                        Message {
                            id: group.get_version(),
                            to: message.from,
                            data: Ack::DeletedChannel(channel_id),
                            ..Message::default()
                        }
                    } else {
                        return Err(anyhow!("Group not found"));
                    }
                }

                /* ===== VOICE ===== */
                Event::JoinChannel => {
                    let channel_id = message.to;

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        group.join_voice_channel(message.from, channel_id);

                        Message {
                            id: group.get_version(),
                            to: message.from,
                            data: Ack::JoinedChannel(channel_id),
                            ..Message::default()
                        }
                    } else {
                        return Err(anyhow!("Group not found"));
                    }
                }

                Event::ExitChannel => {
                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        group.exit_voice_channel(message.from);

                        Message {
                            id: group.get_version(),
                            to: message.from,
                            data: Ack::ExitedChannel,
                            ..Message::default()
                        }
                    } else {
                        return Err(anyhow!("Group not found"));
                    }
                }

                Event::MoveVoice { to } => {
                    let channel_id = to;

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        group.move_voice_channel(message.from, channel_id);

                        Message {
                            id: group.get_version(),
                            to: message.from,
                            data: Ack::MovedVoice(channel_id),
                            ..Message::default()
                        }
                    } else {
                        return Err(anyhow!("Group not found"));
                    }
                }


                /* ===== MODERATION ===== */
                Event::KickUser => {
                    let target_user = message.to;

                    if let Some(group) = state.groups.get(&group_id) {
                        let perms = group.compute_permissions(message.from, None);

                        if !perms.contains(Permissions::KICK_MEMBERS | Permissions::ADMINISTRATOR) {
                            return Err(anyhow!("Unauthorized to kick user"));
                        }
                    } else {
                        return Err(anyhow!("Group not found"));
                    }

                    let _lock = state.group_locks.write(group_id).await;

                    // Gruptan çıkarma işlemi (Kick genellikle soft-delete değil, ilişkiyi silmektir)
                    db::group::remove_user(&state.pool, group_id, target_user).await?;

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        group.users.remove(&target_user);

                        // Kicklenen kullanıcıya da ayrıca bildirim gitmesi gerekebilir,
                        // ancak burada işlem yapan kişiye Ack dönüyoruz.
                        Message {
                            id: group.get_version(),
                            to: message.from,
                            data: Ack::KickedUser(target_user),
                            ..Message::default()
                        }
                    } else {
                        return Err(anyhow!("Group not found"));
                    }
                }

                Event::BanUser => {
                    let target_user = message.to;

                    if let Some(group) = state.groups.get(&group_id) {
                        let perms = group.compute_permissions(message.from, None);
                        if !perms.contains(Permissions::BAN_MEMBERS | Permissions::ADMINISTRATOR) {
                            return Err(anyhow!("Unauthorized to ban user"));
                        }
                    } else {
                        return Err(anyhow!("Group not found"));
                    }

                    let _lock = state.group_locks.write(group_id).await;

                    db::group::ban_user(&state.pool, group_id, target_user).await?;

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        group.users.remove(&target_user);
                        // Ban listesine ekleme logic'i group struct'ında varsa buraya eklenmeli

                        Message {
                            id: group.get_version(),
                            to: message.from,
                            data: Ack::BannedUser(target_user),
                            ..Message::default()
                        }
                    } else {
                        return Err(anyhow!("Group not found"));
                    }
                }*/
                _ => return Err(anyhow!("Invalid event")),
            };

            if let Some(group) = state.groups.get(&group_id) {
                group.notify(ack, state);
            } else {
                return Err(anyhow!("Group not found"));
            }
        }
        _ => {
            return Err(anyhow!("Invalid message type"));
        }
    }
    Ok(())
}
