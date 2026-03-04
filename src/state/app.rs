use crate::id::id;
use crate::lockmap::LockMap;
use crate::message::Snowflake;
use crate::message::service::MessageService;
use crate::state::group::Group;
use crate::state::user;
use dashmap::DashMap;
use sqlx::PgPool;

pub struct AppState {
    pub users: DashMap<id, user::Session, ahash::RandomState>,
    pub user_locks: LockMap<id>,
    pub groups: DashMap<id, Group, ahash::RandomState>,
    pub group_locks: LockMap<id>,
    pub pool: PgPool,
    pub snowflake: Snowflake,
    pub messages: MessageService,
}
