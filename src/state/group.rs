use crate::State;
use crate::id as Id;
use crate::message::Ack;
use crate::message::Message;
use crate::msgpack;
use bitflags::bitflags;
use bytes::Bytes;
use derive_more::Constructor;
use serde::Deserialize;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

bitflags! {
    pub struct Permissions: u64 {
        // ─── Core / Admin ───────────────────
        const ADMINISTRATOR        = 1 << 0;
        const VIEW_AUDIT_LOG       = 1 << 1;
        const MANAGE_GROUP         = 1 << 2;
        const MANAGE_ROLES         = 1 << 3;
        const MANAGE_CHANNELS      = 1 << 4;
        const KICK_MEMBERS         = 1 << 5;
        const BAN_MEMBERS          = 1 << 6;
        const CREATE_INVITE        = 1 << 7;
        const DELETE_INVITE        = 1 << 8;

        // ─── Text ───────────────────────────
        const VIEW_CHANNEL         = 1 << 9;
        const VIEW_MESSAGES        = 1 << 10;
        const SEND_MESSAGE        = 1 << 11;
        const SEND_TTS_MESSAGES    = 1 << 12;
        const MANAGE_MESSAGES      = 1 << 13;
        const EMBED_LINKS          = 1 << 14;
        const ATTACH_FILES         = 1 << 15;
        const READ_MESSAGE_HISTORY = 1 << 16;
        const MENTION_EVERYONE     = 1 << 17;

        // ─── Voice ──────────────────────────
        const CONNECT              = 1 << 18;
        const SPEAK                = 1 << 19;
        const MUTE_MEMBERS         = 1 << 20;
        const DEAFEN_MEMBERS       = 1 << 21;
        const MOVE_MEMBERS         = 1 << 22;
    }
}

type id = Id::id;
type UserId = id;
type RoleId = id;
type ChannelId = id;

#[derive(Serialize, Clone, Constructor)]
pub struct Group {
    pub id: id,
    pub icon: Option<String>,
    pub name: String,
    pub version: id,
    pub owner: id,
    pub members: HashMap<UserId, Member>,
    pub roles: HashMap<RoleId, Role>,
    pub channels: HashMap<ChannelId, Channel>,
    #[serde(skip)]
    pub subscribers: HashSet<UserId>,
}

#[derive(Serialize, Clone, Constructor, Default)]
pub struct Member {
    id: UserId,
    name: Option<String>,
    roles: Vec<RoleId>,
}

#[derive(Serialize, Clone, Constructor)]
pub struct Role {
    id: RoleId,
    name: String,
    position: usize,
    color: String,
    #[serde(skip)]
    permissions: Permissions,
}

#[derive(Serialize, Clone, Constructor)]
pub struct Channel {
    id: ChannelId,
    name: String,
    title: Option<String>,
    position: usize,
    #[serde(flatten)]
    pub r#type: ChannelType,
    #[serde(skip)]
    permission_overrides: Vec<PermissionOverride>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "lowercase", tag = "type", content = "users")]
pub enum ChannelType {
    Voice(HashSet<id>),
    Text,
}

#[derive(Clone, Constructor)]
pub struct PermissionOverride {
    target: OverrideTarget,
    allow: Permissions,
    deny: Permissions,
}

#[derive(Serialize, Clone)]
pub enum OverrideTarget {
    Role(RoleId),
    User(UserId),
}

impl Group {
    pub fn compute_permissions(
        &self,
        user_id: UserId,
        channel_id: Option<ChannelId>,
    ) -> Permissions {
        let Some(user) = self.members.get(&user_id) else {
            return Permissions::empty();
        };

        let mut perms = Permissions::empty();
        for role_id in &user.roles {
            if let Some(role) = self.roles.get(role_id) {
                perms |= role.permissions;
            }
        }

        if perms.contains(Permissions::ADMINISTRATOR) || self.owner == user_id {
            return Permissions::all();
        }

        if let Some(cid) = channel_id {
            if let Some(channel) = self.channels.get(&cid) {
                let mut role_deny = Permissions::empty();
                let mut role_allow = Permissions::empty();

                for ovr in &channel.permission_overrides {
                    if let OverrideTarget::Role(rid) = ovr.target {
                        if user.roles.contains(&rid) {
                            role_deny |= ovr.deny;
                            role_allow |= ovr.allow;
                        }
                    }
                }

                perms &= !role_deny;
                perms |= role_allow;

                let mut user_deny = Permissions::empty();
                let mut user_allow = Permissions::empty();

                for ovr in &channel.permission_overrides {
                    if let OverrideTarget::User(uid) = ovr.target {
                        if uid == user_id {
                            user_deny |= ovr.deny;
                            user_allow |= ovr.allow;
                        }
                    }
                }

                perms &= !user_deny;
                perms |= user_allow;
            }
        }

        perms
    }

    fn next_version(&mut self) -> id {
        self.version += 1;
        self.version
    }

    pub fn get_version(&self) -> id {
        self.version
    }

    pub fn subscribe(&mut self, user_id: id) -> bool {
        self.subscribers.insert(user_id)
    }

    pub fn unsubscribe(&mut self, user_id: id) -> bool {
        self.subscribers.remove(&user_id)
    }

    pub fn notify(&self, message: Message<Ack>, state: &State) {
        let message = Bytes::from(msgpack!(message));
        self.subscribers.iter().for_each(|user_id| {
            if let Some(user) = state.users.get(user_id) {
                user.send_bytes(message.clone());
            }
        });
    }

    pub fn notify_all(&self, message: Message<Ack>, state: &State) {
        let message = Bytes::from(msgpack!(message));
        self.members.keys().for_each(|user_id| {
            if let Some(user) = state.users.get(user_id) {
                user.send_bytes(message.clone());
            }
        });
    }

    pub fn notify_with_permissions(
        &self,
        message: Message<Ack>,
        permissions: Permissions,
        channel_id: Option<ChannelId>,
        state: &State,
    ) {
        let message = Bytes::from(msgpack!(message));
        self.subscribers.iter().for_each(|user_id| {
            if let Some(user) = state.users.get(user_id) {
                if self
                    .compute_permissions(*user_id, channel_id)
                    .contains(permissions)
                {
                    user.send_bytes(message.clone());
                }
            }
        });
    }

    pub fn update_group(&mut self, name: Option<String>, icon: Option<String>) {
        if let Some(n) = name {
            self.name = n;
        }
        if let Some(i) = icon {
            self.icon = Some(i);
        }
        self.next_version();
    }

    // ===== CHANNEL =====

    pub fn create_channel(
        &mut self,
        channel_id: ChannelId,
        name: String,
        is_voice: bool,
        title: Option<String>,
    ) -> usize {
        let position = self.channels.len() + 1;

        let channel = Channel {
            id: channel_id,
            name,
            title,
            position,
            r#type: if is_voice {
                ChannelType::Voice(HashSet::new())
            } else {
                ChannelType::Text
            },
            permission_overrides: Vec::new(),
        };

        self.channels.insert(channel_id, channel);
        self.next_version();

        position
    }

    pub fn update_channel(
        &mut self,
        channel_id: ChannelId,
        name: Option<String>,
        title: Option<String>,
        position: Option<usize>,
    ) {
        let old_pos = self.channels.get(&channel_id).map(|c| c.position);

        if let (Some(new), Some(old)) = (position, old_pos) {
            for c in self.channels.values_mut().filter(|c| c.id != channel_id) {
                if c.position > old && c.position <= new {
                    c.position -= 1;
                } else if c.position >= new && c.position < old {
                    c.position += 1;
                }
            }
        }

        if let Some(channel) = self.channels.get_mut(&channel_id) {
            if let Some(n) = name {
                channel.name = n;
            }
            channel.title = title;
            if let Some(p) = position {
                channel.position = p;
            }
            self.next_version();
        }
    }

    pub fn delete_channel(&mut self, channel_id: ChannelId) {
        if self.channels.remove(&channel_id).is_some() {
            self.next_version();
        }
    }

    // ===== ROLE =====
    pub fn create_role(
        &mut self,
        role_id: id,
        name: String,
        color: String,
        permissions: Permissions,
    ) {
        let position = self.roles.values().map(|r| r.position).max().unwrap_or(0) + 1;
        let role = Role {
            id: role_id,
            name,
            color,
            permissions,
            position,
        };
        self.roles.insert(role_id, role);
        self.next_version();
    }

    pub fn update_role(
        &mut self,
        role_id: id,
        name: Option<String>,
        position: Option<usize>,
        color: Option<String>,
        permissions: Option<Permissions>,
    ) {
        let old_pos = self.roles.get(&role_id).map(|r| r.position);

        if let (Some(new), Some(old)) = (position, old_pos) {
            for r in self.roles.values_mut().filter(|r| r.id != role_id) {
                if r.position > old && r.position <= new {
                    r.position -= 1;
                } else if r.position >= new && r.position < old {
                    r.position += 1;
                }
            }
        }

        if let Some(role) = self.roles.get_mut(&role_id) {
            if let Some(n) = name {
                role.name = n;
            }
            if let Some(p) = position {
                role.position = p;
            }
            if let Some(c) = color {
                role.color = c;
            }
            if let Some(p) = permissions {
                role.permissions = p;
            }
            self.next_version();
        }
    }

    pub fn delete_role(&mut self, role_id: RoleId) {
        if self.roles.remove(&role_id).is_some() {
            self.members.iter_mut().for_each(|(_, u)| {
                u.roles.retain(|&r| r != role_id);
            });
            self.next_version();
        }
    }

    pub fn assign_role(&mut self, user_id_param: UserId, role_id: RoleId) {
        if self.roles.contains_key(&role_id) {
            if let Some(u) = self.members.get_mut(&user_id_param) {
                u.roles.push(role_id);
                self.next_version();
            }
        }
    }

    pub fn remove_role(&mut self, user_id_param: UserId, role_id: RoleId) {
        if let Some(u) = self.members.get_mut(&user_id_param) {
            u.roles.retain(|&r| r != role_id);
            self.next_version();
        }
    }
}
