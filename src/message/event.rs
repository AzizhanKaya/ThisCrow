use crate::db::{self};
use crate::id::id;
use crate::message::data::Data;
use crate::message::{Ack, MessageType};
use crate::state::group::{ChannelType, Group, OverrideTarget};
use crate::state::group::{Permissions, WatchParty};
use crate::state::user::{self, Voice, VoiceType};
use crate::{State, message::Message};
use anyhow::Result;
use dashmap::Entry;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "event", content = "payload")]
pub enum Event {
    /* ===== USER ===== */
    UpdateUser {
        name: Option<String>,
        avatar: Option<String>,
        banner: Option<String>,
    },
    ChangeStatus(user::Status),

    /* ===== ACTIVITY ===== */
    Music(MusicEvent),
    Game(GameEvent),

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
    MoveGroup {
        position: usize,
    },

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
    SetPermissionOverride {
        target: OverrideTarget,
        allow: u64,
        deny: u64,
    },
    DeletePermissionOverride {
        target: OverrideTarget,
    },

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
    IceCandidate {
        candidate: Option<String>,
        #[serde(rename = "sdpMid")]
        sdp_mid: Option<String>,
        #[serde(rename = "sdpMLineIndex")]
        sdp_mline_index: Option<u16>,
        #[serde(rename = "usernameFragment")]
        username_fragment: Option<String>,
    },

    // ==== WATCH PARTY ====
    JoinParty,
    LeaveParty,
    Watch(id),
    JumpTo {
        offset: f64,
        play: bool,
    },
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "music", content = "payload")]
pub enum MusicEvent {
    Playing(user::Music),
    Seek(f64),
    Paused,
    Stopped,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "game", content = "payload")]
pub enum GameEvent {
    Playing(user::Game),
    Stopped,
}

pub async fn handle_event(
    message: Message<Event>,
    connection_id: usize,
    state: &State,
) -> Result<()> {
    match message.r#type {
        MessageType::Info => {
            match message.data {
                /* ===== USER ===== */
                Event::UpdateUser {
                    name,
                    avatar,
                    banner,
                } => {
                    db::user::update_user(
                        &state.pool,
                        message.from,
                        name.clone(),
                        avatar.clone(),
                        banner.clone(),
                    )
                    .await?;

                    if let Some(mut user) = state.users.get_mut(&message.from) {
                        if let Some(name) = name.clone() {
                            user.state.name = name;
                        }
                        if let Some(avatar) = avatar.clone() {
                            user.state.avatar = Some(avatar);
                        }
                        if let Some(banner) = banner.clone() {
                            user.state.banner = Some(banner);
                        }

                        let ack = Message {
                            id: message.id,
                            from: message.from,
                            data: Ack::UpdatedUser {
                                name,
                                avatar,
                                banner,
                            },
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

                /* ===== ACTIVITY ===== */
                Event::Music(music_event) => {
                    if let Some(mut user) = state.users.get_mut(&message.from) {
                        let music_activity = user
                            .state
                            .activities
                            .iter_mut()
                            .find(|a| matches!(a, user::Activity::Music(_)));

                        match &music_event {
                            MusicEvent::Playing(music) => {
                                let music = music.clone();
                                if let Some(user::Activity::Music(m)) = music_activity {
                                    *m = music;
                                } else {
                                    user.state.activities.push(user::Activity::Music(music));
                                }
                            }
                            MusicEvent::Seek(offset) => {
                                if let Some(user::Activity::Music(music)) = music_activity {
                                    if music.paused {
                                        let now = std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap()
                                            .as_millis()
                                            as f64;
                                        music.offset = now - offset;
                                    } else {
                                        music.offset = *offset;
                                    }
                                }
                            }
                            MusicEvent::Paused => {
                                if let Some(user::Activity::Music(music)) = music_activity {
                                    let now = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap()
                                        .as_millis()
                                        as f64;

                                    music.offset = now - music.offset;
                                    music.paused = true;
                                }
                            }
                            MusicEvent::Stopped => {
                                user.state
                                    .activities
                                    .retain(|a| !matches!(a, user::Activity::Music(_)));
                            }
                        }

                        let ack = Message {
                            id: message.id,
                            from: message.from,
                            data: Ack::MusicActivity(music_event),
                            ..Message::default()
                        };

                        let user = user.downgrade();

                        user.send_message_all(ack, &state);
                    }
                }

                Event::Game(game_event) => {
                    if let Some(mut user) = state.users.get_mut(&message.from) {
                        let game_activity = user
                            .state
                            .activities
                            .iter_mut()
                            .find(|a| matches!(a, user::Activity::Game(_)));

                        match &game_event {
                            GameEvent::Playing(game) => {
                                let game = game.clone();
                                if let Some(user::Activity::Game(g)) = game_activity {
                                    *g = game;
                                } else {
                                    user.state.activities.push(user::Activity::Game(game));
                                }
                            }
                            GameEvent::Stopped => {
                                user.state
                                    .activities
                                    .retain(|a| !matches!(a, user::Activity::Game(_)));
                            }
                        }

                        let ack = Message {
                            id: message.id,
                            from: message.from,
                            data: Ack::GameActivity(game_event),
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
                        user.send_message(ack);
                    }
                }

                Event::Offer(_) | Event::Answer(_) | Event::IceCandidate { .. } => {
                    if let Some(user) = state.users.get(&message.to) {
                        user.send_message(message);
                    }
                }

                Event::JoinVoice => {
                    if state
                        .users
                        .get(&message.from)
                        .map_or(false, |u| u.state.voice.is_some())
                    {
                        anyhow::bail!("Already in a voice channel");
                    }

                    let existing_id = state.users.get(&message.to).and_then(|other| {
                        match other.state.voice.as_ref()?.r#type {
                            VoiceType::Direct { user, message_id } if user == message.from => {
                                Some(message_id)
                            }
                            _ => None,
                        }
                    });

                    let (call_message_id, is_new) = match existing_id {
                        Some(i) => (i, false),
                        None => (state.snowflake.generate(), true),
                    };

                    let call = Message {
                        id: call_message_id,
                        from: message.from,
                        to: message.to,
                        data: Data::Call { end_time: None },
                        r#type: MessageType::Direct,
                    };

                    if is_new {
                        state.messages.write(call.clone().try_into()?).await?;
                    }

                    let ack = Message {
                        id: message.id,
                        to: message.from,
                        data: Ack::JoinedVoice(message.to),
                        ..Message::default()
                    };

                    if let Some(mut user) = state.users.get_mut(&message.from) {
                        user.state.voice = Some(Voice {
                            connection_id,
                            r#type: VoiceType::Direct {
                                user: message.to,
                                message_id: call_message_id,
                            },
                        });
                        user.send_message(ack.clone());
                        if is_new {
                            user.send_message(call.clone());
                        }
                    }

                    state
                        .voice_direct
                        .entry(message.to)
                        .or_default()
                        .insert(message.from);

                    if let Some(other) = state.users.get(&message.to) {
                        other.send_message(ack);
                        if is_new {
                            other.send_message(call);
                        }
                    }
                }

                Event::ExitVoice => {
                    let call_message_id = state
                        .users
                        .get(&message.from)
                        .and_then(|user| match user.state.voice.as_ref()?.r#type {
                            VoiceType::Direct {
                                user: other,
                                message_id,
                            } if other == message.to => Some(message_id),
                            _ => None,
                        })
                        .ok_or_else(|| anyhow::anyhow!("Not in a voice channel"))?;

                    if let Some(mut user) = state.users.get_mut(&message.from) {
                        user.state.voice = None;
                    }

                    if let Entry::Occupied(mut entry) = state.voice_direct.entry(message.to) {
                        entry.get_mut().remove(&message.from);
                        if entry.get().is_empty() {
                            entry.remove();
                        }
                    }

                    let exit_ack = Message {
                        id: message.id,
                        to: message.from,
                        data: Ack::ExitedVoice(message.to),
                        ..Message::default()
                    };

                    if let Some(user) = state.users.get(&message.from) {
                        user.send_message(exit_ack.clone());
                    }
                    if let Some(other) = state.users.get(&message.to) {
                        other.send_message(exit_ack);
                    }

                    let other_is_in = state.users.get(&message.to).map_or(false, |other| {
                        matches!(
                            other.state.voice.as_ref().map(|v| &v.r#type),
                            Some(VoiceType::Direct { user, message_id })
                                if *user == message.from && *message_id == call_message_id
                        )
                    });

                    if !other_is_in {
                        let mut stored = state.messages.get(call_message_id).await?;
                        stored.data = Data::Call {
                            end_time: Some(
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap()
                                    .as_millis() as f64,
                            ),
                        };
                        state.messages.overwrite(stored.clone()).await?;

                        let overwrite_ack = Message {
                            id: message.id,
                            to: message.from,
                            data: Ack::Overwritten(Box::new(stored)),
                            ..Message::default()
                        };

                        if let Some(user) = state.users.get(&message.from) {
                            user.send_message(overwrite_ack.clone());
                        }
                        if let Some(user) = state.users.get(&message.to) {
                            user.send_message(overwrite_ack);
                        }
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

                            let group = group.downgrade();

                            let ack = Message {
                                id: message.id,
                                from: group_id,
                                to: message.from,
                                data: Ack::Subscribed(Box::new(group.clone())),
                                ..Message::default()
                            };

                            user.send_message(ack);
                        }
                    } else {
                        anyhow::bail!("Group not found");
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

                Event::MoveGroup { position } => {
                    db::group::update_group_user_position(
                        &state.pool,
                        message.from,
                        group_id,
                        position as i16,
                    )
                    .await?;

                    if let Some(user) = state.users.get(&message.from) {
                        let ack = Message {
                            id: message.id,
                            from: group_id,
                            to: message.from,
                            data: Ack::MovedGroup { position },
                            ..Message::default()
                        };
                        user.send_message(ack);
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

                        let group = group.downgrade();

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

                        let group = group.downgrade();

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

                        let group = group.downgrade();

                        group.notify(ack, &state);
                    }
                }

                Event::SetPermissionOverride {
                    target,
                    allow,
                    deny,
                } => {
                    let channel_id = message.to;

                    if let Some(group) = state.groups.get(&group_id) {
                        let perms = group.compute_permissions(message.from, Some(channel_id));
                        if !perms.intersects(Permissions::MANAGE_ROLES | Permissions::ADMINISTRATOR)
                        {
                            anyhow::bail!("Unauthorized to set permission override");
                        }
                        if !group.channels.contains_key(&channel_id) {
                            anyhow::bail!("Channel not found");
                        }
                    } else {
                        anyhow::bail!("Group not found");
                    }

                    let _lock = state.group_locks.write(group_id).await;

                    db::group::set_permission_override(
                        &state.pool,
                        group_id,
                        channel_id,
                        &target,
                        allow,
                        deny,
                    )
                    .await?;

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        group.set_permission_override(
                            channel_id,
                            target.clone(),
                            Permissions::from_bits_truncate(allow),
                            Permissions::from_bits_truncate(deny),
                        );

                        let ack = Message {
                            id: message.id,
                            from: group_id,
                            to: channel_id,
                            data: Ack::SetPermissionOverride {
                                target,
                                allow,
                                deny,
                            },
                            ..Message::default()
                        };

                        let group = group.downgrade();
                        group.notify(ack, &state);
                    }
                }

                Event::DeletePermissionOverride { target } => {
                    let channel_id = message.to;

                    if let Some(group) = state.groups.get(&group_id) {
                        let perms = group.compute_permissions(message.from, Some(channel_id));
                        if !perms.intersects(Permissions::MANAGE_ROLES | Permissions::ADMINISTRATOR)
                        {
                            anyhow::bail!("Unauthorized to delete permission override");
                        }
                        if !group.channels.contains_key(&channel_id) {
                            anyhow::bail!("Channel not found");
                        }
                    } else {
                        anyhow::bail!("Group not found");
                    }

                    let _lock = state.group_locks.write(group_id).await;

                    db::group::delete_permission_override(&state.pool, channel_id, &target).await?;

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        group.remove_permission_override(channel_id, &target);

                        let ack = Message {
                            id: message.id,
                            from: group_id,
                            to: channel_id,
                            data: Ack::DeletedPermissionOverride { target },
                            ..Message::default()
                        };

                        let group = group.downgrade();
                        group.notify(ack, &state);
                    }
                }

                Event::JoinVoice => {
                    let channel_id = message.to;

                    if let Some(group) = state.groups.get(&group_id) {
                        let perms = group.compute_permissions(message.from, Some(channel_id));
                        if !perms.contains(Permissions::CONNECT) {
                            anyhow::bail!("Unauthorized to join channel");
                        }
                    } else {
                        anyhow::bail!("Group not found");
                    }

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        let Some(channel) = group.channels.get_mut(&channel_id) else {
                            anyhow::bail!("Channel not found");
                        };

                        if let Some(user) = state.users.get(&message.from) {
                            if user.state.voice.is_some() {
                                anyhow::bail!("User is already in a voice channel");
                            }
                        }

                        match channel.r#type {
                            ChannelType::Voice { ref mut users, .. } => {
                                users.insert(message.from);
                            }
                            ChannelType::Text => {
                                anyhow::bail!("Cannot join a text channel");
                            }
                        }

                        let group = group.downgrade();

                        if let Some(mut user) = state.users.get_mut(&message.from) {
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
                    } else {
                        anyhow::bail!("Group not found");
                    }
                }

                Event::ExitVoice => {
                    let channel_id = message.to;

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        let Some(channel) = group.channels.get_mut(&channel_id) else {
                            anyhow::bail!("Channel not found");
                        };

                        match channel.r#type {
                            ChannelType::Voice { ref mut users, .. } => {
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
                    } else {
                        anyhow::bail!("Group not found");
                    }
                }

                Event::MoveToVoice(channel_id) => {
                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        let Some(channel) = group.channels.get_mut(&channel_id) else {
                            anyhow::bail!("Channel not found");
                        };

                        match channel.r#type {
                            ChannelType::Voice { ref mut users, .. } => {
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

                        let group = group.downgrade();

                        group.notify(ack, &state);
                    } else {
                        anyhow::bail!("Group not found");
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

                        let group = group.downgrade();

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

                    if role == crate::id::id(0) {
                        if name.is_some() || color.is_some() || position.is_some() {
                            anyhow::bail!("Everyone role only supports permission updates");
                        }
                        let Some(perms_bits) = permissions else {
                            anyhow::bail!("permissions required for everyone role");
                        };

                        db::group::update_everyone_permissions(&state.pool, group_id, perms_bits)
                            .await?;

                        if let Some(mut group) = state.groups.get_mut(&group_id) {
                            group.update_everyone(Permissions::from_bits_truncate(perms_bits));

                            let ack = Message {
                                id: message.id,
                                from: group_id,
                                to: crate::id::id(0),
                                data: Ack::UpdatedRole {
                                    name: None,
                                    color: None,
                                    permissions: Some(perms_bits),
                                    position: None,
                                },
                                ..Message::default()
                            };

                            let group = group.downgrade();
                            group.notify(ack, &state);
                        }
                        return Ok(());
                    }

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
                                position,
                            },
                            ..Message::default()
                        };

                        let group = group.downgrade();

                        group.notify(ack, &state);
                    }
                }

                Event::DeleteRole { role } => {
                    if role == crate::id::id(0) {
                        anyhow::bail!("Cannot delete everyone role");
                    }
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

                        let group = group.downgrade();

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

                        let group = group.downgrade();

                        group.notify(ack, &state);
                    }
                }

                Event::RemoveRole { user, role } => {
                    if let Some(group) = state.groups.get(&group_id) {
                        let perms = group.compute_permissions(message.from, None);
                        if !perms.contains(Permissions::MANAGE_ROLES | Permissions::ADMINISTRATOR) {
                            anyhow::bail!("Unauthorized to remove role");
                        }
                    } else {
                        anyhow::bail!("Group not found");
                    }

                    let _lock = state.group_locks.write(group_id).await;

                    db::group::remove_role(&state.pool, user, role, group_id).await?;

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        group.remove_role(user, role);

                        let ack = Message {
                            id: message.id,
                            from: group_id,
                            to: user,
                            data: Ack::RemovedRole { role_id: role },
                            ..Message::default()
                        };

                        let group = group.downgrade();

                        group.notify(ack, &state);
                    }
                }

                Event::JoinParty => {
                    let _lock = state.group_locks.write(group_id).await;

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        let Some(channel) = group.channels.get_mut(&message.to) else {
                            anyhow::bail!("Channel not found");
                        };

                        let ChannelType::Voice {
                            users, watch_party, ..
                        } = &mut channel.r#type
                        else {
                            anyhow::bail!("Channel is not a voice channel");
                        };

                        if !users.contains(&message.from) {
                            anyhow::bail!("Not in voice channel");
                        }

                        match watch_party {
                            None => {
                                *watch_party = Some(WatchParty {
                                    host: message.from,
                                    users: HashSet::from([message.from]),
                                    ..Default::default()
                                });
                            }
                            Some(party) => {
                                party.users.insert(message.from);
                            }
                        }

                        let ack = Message {
                            id: message.id,
                            from: group_id,
                            to: message.from,
                            data: Ack::JoinedParty,
                            ..Message::default()
                        };

                        let group = group.downgrade();

                        group.notify(ack, &state);
                    }
                }

                Event::LeaveParty => {
                    let _lock = state.group_locks.write(group_id).await;

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        let Some(channel) = group.channels.get_mut(&message.to) else {
                            anyhow::bail!("Channel not found");
                        };

                        let ChannelType::Voice { watch_party, .. } = &mut channel.r#type else {
                            anyhow::bail!("Channel is not a voice channel");
                        };

                        let Some(party) = watch_party.as_mut() else {
                            anyhow::bail!("Party not found");
                        };

                        party.users.remove(&message.from);

                        if party.users.is_empty() {
                            *watch_party = None;
                        } else if party.host == message.from {
                            party.host = *party
                                .users
                                .iter()
                                .min()
                                .expect("Users list is guaranteed to be non-empty");
                        }

                        let ack = Message {
                            id: message.id,
                            from: group_id,
                            to: message.from,
                            data: Ack::LeftParty,
                            ..Message::default()
                        };

                        let group = group.downgrade();

                        group.notify(ack, &state);
                    }
                }

                Event::Watch(video_id) => {
                    let _lock = state.group_locks.write(group_id).await;

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        let Some(channel) = group.channels.get_mut(&message.to) else {
                            anyhow::bail!("Channel not found");
                        };

                        let ChannelType::Voice {
                            watch_party: Some(ref mut watch_party),
                            ..
                        } = channel.r#type
                        else {
                            anyhow::bail!("Party not found");
                        };

                        if watch_party.host != message.from {
                            anyhow::bail!("You are not the host");
                        }

                        watch_party.video = video_id;
                        watch_party.offset = 0.0;
                        watch_party.playing = false;

                        let ack = Message {
                            id: message.id,
                            from: group_id,
                            to: message.from,
                            data: Ack::Watching { video: video_id },
                            ..Message::default()
                        };

                        let group = group.downgrade();

                        group.notify(ack, &state);
                    }
                }

                Event::JumpTo { offset, play } => {
                    let _lock = state.group_locks.write(group_id).await;

                    if let Some(mut group) = state.groups.get_mut(&group_id) {
                        let Some(channel) = group.channels.get_mut(&message.to) else {
                            anyhow::bail!("Channel not found");
                        };

                        let ChannelType::Voice {
                            watch_party: Some(ref mut watch_party),
                            ..
                        } = channel.r#type
                        else {
                            anyhow::bail!("Party not found");
                        };

                        if watch_party.host != message.from {
                            anyhow::bail!("You are not the host");
                        }

                        if watch_party.video == id(0) {
                            anyhow::bail!("No video is being watched");
                        }

                        watch_party.offset = offset;
                        watch_party.playing = play;

                        let ack = Message {
                            id: message.id,
                            from: group_id,
                            to: message.from,
                            data: Ack::JumpedTo { offset, play },
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
