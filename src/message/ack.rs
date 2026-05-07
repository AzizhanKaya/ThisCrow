use crate::db::message::StoredMessage;
use crate::id::id;
use crate::message::event;
use crate::message::snowflake::snowflake_id;
use crate::state::group::{Group, WatchParty};
use crate::state::user;
use serde::Serialize;

#[serde_with::skip_serializing_none]
#[derive(Serialize, Clone, Default)]
#[serde(tag = "ack", content = "payload", rename_all = "snake_case")]
pub enum Ack {
    #[default]
    None,
    Error(String),

    // Message
    Received(snowflake_id),
    Deleted(snowflake_id),
    Overwritten(Box<StoredMessage>),

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
    Subscribed(Box<Group>),
    Unsubscribed,
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
        permissions: u64,
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
        permissions: Option<u64>,
        color: Option<String>,
    },
    DeletedGroup,
    DeletedChannel,
    DeletedRole,

    // VOICE
    JoinedVoice(id),
    ExitedVoice(id),
    MovedToVoice(id),

    // ==== WATCH PARTY ====
    CreatedParty(id),
    JoinedParty {
        channel: id,
        state: WatchParty,
    },
    LeftParty {
        channel: id,
        new_host: Option<id>,
    },
    Watching(id),
    UnWatched,
    JumpedTo {
        offset: f64,
        play: bool,
    },
}
