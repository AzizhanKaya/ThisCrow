use crate::state::group::Group;
use crate::state::user;
use crate::{id::id as Id, state::group::ChannelType};
use serde::Serialize;

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "ack", content = "payload")]
pub enum Ack {
    #[default]
    None,
    Error(String),
    Received(Id),

    // USER
    Initialized(user::State),
    ChangedStatus(user::Status),
    AssignedRole(Id),
    AddedFriend(Id),
    ReceivedFriendRequest(Id),
    SentFriendRequest(Id),
    DeletedFriend(Id),
    UpdatedUser {
        name: Option<String>,
        avatar: Option<String>,
    },

    // GROUP
    Subscribed(Group),
    Unsubscribed(Id),
    AddedMember(Id),
    RemovedMember(Id),
    CreatedGroup {
        id: Id,
        name: String,
        icon: Option<String>,
    },
    CreatedChannel {
        id: Id,
        name: String,
        kind: ChannelType,
    },
    CreatedRole {
        id: Id,
        name: String,
        permissions: u64,
        color: String,
    },
    UpdatedGroup {
        id: Id,
        name: Option<String>,
        icon: Option<String>,
    },
    UpdatedChannel {
        id: Id,
        name: Option<String>,
    },
    UpdatedRole {
        id: Id,
        name: Option<String>,
        permissions: Option<u64>,
        color: Option<String>,
    },
    DeletedGroup(Id),
    DeletedChannel(Id),
    DeletedRole(Id),
}
