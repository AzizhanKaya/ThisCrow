use crate::state::group::Group;
use crate::state::user;
use crate::{id::id as Id, state::group::ChannelType};
use serde::Serialize;

#[serde_with::skip_serializing_none]
#[derive(Serialize, Clone, Default)]
#[serde(tag = "ack", content = "payload", rename_all = "snake_case")]
pub enum Ack {
    #[default]
    None,
    Error(String),
    Received(Id),

    // USER
    Initialized(Box<user::State>),
    ChangedStatus(user::Status),
    AssignedRole,
    AddedFriend,
    ReceivedFriendRequest,
    SentFriendRequest,
    DeletedFriend,
    UpdatedUser {
        name: Option<String>,
        avatar: Option<String>,
    },

    // GROUP
    Subscribed(Box<Group>),
    Unsubscribed,
    AddedMember,
    RemovedMember,
    CreatedGroup {
        name: String,
        icon: Option<String>,
        description: Option<String>,
    },
    CreatedChannel {
        name: String,
        position: usize,
        r#type: ChannelType,
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
}
