use crate::State;
use crate::db;
use crate::id::id;
use crate::message::Ack;
use crate::message::Message;
use bitflags::bitflags;
use bytes::Bytes;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
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

        // ─── Text ───────────────────────────
        const VIEW_CHANNEL         = 1 << 8;
        const VIEW_MESSAGES        = 1 << 9;
        const SEND_MESSAGE        = 1 << 10;
        const SEND_TTS_MESSAGES    = 1 << 11;
        const MANAGE_MESSAGES      = 1 << 12;
        const EMBED_LINKS          = 1 << 13;
        const ATTACH_FILES         = 1 << 14;
        const READ_MESSAGE_HISTORY = 1 << 15;
        const MENTION_EVERYONE     = 1 << 16;

        // ─── Voice ──────────────────────────
        const CONNECT              = 1 << 17;
        const SPEAK                = 1 << 18;
        const MUTE_MEMBERS         = 1 << 19;
        const DEAFEN_MEMBERS       = 1 << 20;
        const MOVE_MEMBERS         = 1 << 21;
    }
}

type UserId = id;
type RoleId = id;
type ChannelId = id;

#[derive(Serialize, Clone)]
pub struct Group {
    pub id: id,
    pub icon: Option<String>,
    pub name: String,
    pub version: id,
    pub users: HashMap<UserId, User>,
    roles: HashMap<RoleId, Role>,
    channels: HashMap<ChannelId, Channel>,
    #[serde(skip)]
    pub subscribers: HashSet<UserId>,
}

impl From<db::group::Group> for Group {
    fn from(value: db::group::Group) -> Self {
        Self {
            id: value.id,
            icon: value.icon,
            name: value.name,
            version: id(0),
            users: HashMap::new(),
            roles: HashMap::new(),
            channels: HashMap::new(),
            subscribers: HashSet::new(),
        }
    }
}

#[derive(Serialize, Clone)]
pub struct User {
    roles: Vec<RoleId>,
}

#[derive(Serialize, Clone)]
struct Role {
    id: RoleId,
    name: String,
    color: String,
    #[serde(skip)]
    permissions: Permissions,
}

#[derive(Serialize, Clone)]
struct Channel {
    id: ChannelId,
    name: String,
    order: usize,
    r#type: ChannelType,
    #[serde(skip)]
    permission_overrides: Vec<PermissionOverride>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "lowercase")]
pub enum ChannelType {
    Voice,
    Chat,
}

#[derive(Clone)]
struct PermissionOverride {
    target: OverrideTarget,
    allow: Permissions,
    deny: Permissions,
}

#[derive(Serialize, Clone)]
enum OverrideTarget {
    Role(RoleId),
    User(UserId),
}

impl Group {
    pub fn compute_permissions(
        &self,
        user_id: UserId,
        channel_id: Option<ChannelId>,
    ) -> Permissions {
        let Some(user) = self.users.get(&user_id) else {
            return Permissions::empty();
        };

        let mut perms = Permissions::empty();
        for role_id in &user.roles {
            if let Some(role) = self.roles.get(role_id) {
                perms |= role.permissions;
            }
        }

        if perms.contains(Permissions::ADMINISTRATOR) {
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
        self.version.add(1)
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
        let message = Bytes::from(json!(message).to_string());
        self.subscribers.iter().for_each(|user_id| {
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
        let message = Bytes::from(json!(message).to_string());
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

    // ===== Channel Management =====
    pub fn create_channel(&mut self, channel_id: ChannelId, name: String, r#type: ChannelType) {
        let order = self.channels.len();
        let channel = Channel {
            id: channel_id,
            name,
            order,
            r#type,
            permission_overrides: Vec::new(),
        };
        self.channels.insert(channel_id, channel);
        self.next_version();
    }

    pub fn update_channel(
        &mut self,
        channel_id: ChannelId,
        name: Option<String>,
        order: Option<u64>,
    ) {
        if let Some(ch) = self.channels.get_mut(&channel_id) {
            if let Some(n) = name {
                ch.name = n;
            }
            if let Some(o) = order {
                ch.order = o as usize;
            }
            self.next_version();
        }
    }

    pub fn delete_channel(&mut self, channel_id: ChannelId) {
        if self.channels.remove(&channel_id).is_some() {
            self.next_version();
        }
    }

    // ===== Role Management =====
    pub fn create_role(
        &mut self,
        role_id: id,
        name: String,
        color: String,
        permissions: Permissions,
    ) {
        let role = Role {
            id: role_id,
            name,
            color,
            permissions,
        };
        self.roles.insert(role_id, role);
        self.next_version();
    }

    pub fn update_role(
        &mut self,
        role_id: id,
        name: Option<String>,
        color: Option<String>,
        permissions: Option<Permissions>,
    ) {
        if let Some(role) = self.roles.get_mut(&role_id) {
            if let Some(n) = name {
                role.name = n;
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
            self.users.iter_mut().for_each(|(_, u)| {
                u.roles.retain(|&r| r != role_id);
            });
            self.next_version();
        }
    }

    pub fn assign_role(&mut self, user_id_param: UserId, role_id: RoleId) {
        if self.roles.contains_key(&role_id) {
            if let Some(u) = self.users.get_mut(&user_id_param) {
                u.roles.push(role_id);
                self.next_version();
            }
        }
    }

    pub fn remove_role(&mut self, user_id_param: UserId, role_id: RoleId) {
        if let Some(u) = self.users.get_mut(&user_id_param) {
            u.roles.retain(|&r| r != role_id);
            self.next_version();
        }
    }
}
