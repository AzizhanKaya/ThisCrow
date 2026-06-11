use crate::db::message::StoredMessage;
use crate::id::id;
use crate::message::event;
use crate::message::snowflake::snowflake_id;
use crate::state::group::{Group, OverrideTarget, Permissions};
use crate::state::user;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize, Clone, Debug)]
pub struct VoiceStateSnapshot {
    pub mute: bool,
    pub deafen: bool,
}

#[serde_with::skip_serializing_none]
#[derive(Serialize, Clone, Default)]
#[serde(tag = "ack", content = "payload", rename_all = "snake_case")]
pub enum Ack {
    #[default]
    None,
    Error(String),
    MessageError(String),

    // Message
    Received(snowflake_id),
    Deleted(snowflake_id),
    Overwritten(Box<StoredMessage>),
    Reacted {
        message: snowflake_id,
        reaction: char,
    },
    RemovedReaction {
        message: snowflake_id,
        reaction: char,
    },

    // USER
    Initialized(Box<user::State>),
    ChangedStatus(user::Status),
    MusicActivity(event::MusicEvent),
    GameActivity(event::GameEvent),
    AddedFriend,
    ReceivedFriendRequest,
    SentFriendRequest,
    DeletedFriend,
    UpdatedUser {
        name: Option<String>,
        avatar: Option<String>,
        banner: Option<String>,
    },

    // GROUP
    Subscribed {
        group: Box<Group>,
        permissions: Permissions,
        channel_permissions: HashMap<id, Permissions>,
        voice_states: HashMap<id, VoiceStateSnapshot>,
    },
    Unsubscribed,
    PermissionsChanged(Permissions),
    ChannelPermissionsChanged(Permissions),
    JoinedMember,
    LeftMember,
    CreatedGroup {
        name: String,
        icon: Option<String>,
        description: Option<String>,
    },
    CreatedChannel {
        name: String,
        position: usize,
        is_voice: bool,
        title: Option<String>,
    },
    AssignedRole {
        role_id: id,
    },
    RemovedRole {
        role_id: id,
    },
    CreatedRole {
        name: String,
        color: String,
    },
    UpdatedGroup {
        name: Option<String>,
        description: Option<String>,
        icon: Option<String>,
    },
    UpdatedChannel {
        name: Option<String>,
        #[serialize_always]
        title: Option<String>,
        position: Option<usize>,
    },
    UpdatedRole {
        name: Option<String>,
        permissions: Option<Permissions>,
        color: Option<String>,
        position: Option<usize>,
    },
    DeletedGroup,
    DeletedChannel,
    DeletedRole,
    SetPermissionOverride {
        target: OverrideTarget,
        allow: u64,
        deny: u64,
    },
    DeletedPermissionOverride {
        target: OverrideTarget,
    },
    MovedGroup {
        position: usize,
    },

    // VOICE
    JoinedVoice {
        channel_id: id,
        mute: bool,
        deafen: bool,
    },
    ExitedVoice(id),
    MovedToVoice(id),
    Muted(bool),
    Deafened(bool),

    // ==== WATCH PARTY ====
    JoinedParty(id),
    LeftParty(id),
    Watching {
        video: id,
        title: String,
        duration: f64,
        thumbnail: String,
    },
    JumpedTo {
        offset: f64,
        play: bool,
    },
}
