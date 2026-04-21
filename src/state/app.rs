use crate::id::id;
use crate::lockmap::LockMap;
use crate::message::service::MessageService;
use crate::message::snowflake::SnowflakeGenerator;
use crate::state::group::Group;
use crate::state::user;
use dashmap::DashMap;
use nohash_hasher::BuildNoHashHasher;
use sqlx::PgPool;

pub struct AppState {
    pub users: DashMap<id, user::Session, BuildNoHashHasher<id>>,
    pub user_locks: LockMap<id>,
    pub groups: DashMap<id, Group, BuildNoHashHasher<id>>,
    pub group_locks: LockMap<id>,
    pub pool: PgPool,
    pub snowflake: SnowflakeGenerator,
    pub messages: MessageService,
}
