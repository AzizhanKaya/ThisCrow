use crate::db::message::MessageStore;
use crate::db::message::StoredMessage;
use crate::id::id;
use crate::message::snowflake::snowflake_id;
use anyhow::Result;
use chrono::{DateTime, Utc};

pub struct MessageService {
    store: MessageStore,
}

impl MessageService {
    pub fn new(store: MessageStore) -> Self {
        Self { store }
    }

    pub fn get(&self, message_id: snowflake_id) -> Result<StoredMessage> {
        self.store.get(message_id)
    }

    pub fn save_message(&self, msg: StoredMessage) -> Result<()> {
        self.store.write(msg)
    }

    pub fn overwrite(&self, message: StoredMessage) -> Result<()> {
        self.store.overwrite(message)
    }

    pub fn delete(&self, message_id: snowflake_id) -> Result<()> {
        self.store.delete(message_id)
    }

    pub fn remove_dm(&self, from: id, to: id) -> Result<()> {
        self.store.remove_dm(from, to)
    }

    pub fn get_channel_messages(
        &self,
        channel_id: id,
        start: Option<DateTime<Utc>>,
        end: DateTime<Utc>,
        len: Option<i64>,
    ) -> Result<Vec<StoredMessage>> {
        self.store.get_channel_messages(channel_id, len, start, end)
    }

    pub fn get_direct_messages(
        &self,
        user1: id,
        user2: id,
        start: Option<DateTime<Utc>>,
        end: DateTime<Utc>,
        len: Option<i64>,
    ) -> Result<Vec<StoredMessage>> {
        self.store
            .get_direct_messages(user1, user2, len, start, end)
    }

    pub fn get_dms(&self, user_id: id) -> Result<Vec<(id, snowflake_id)>> {
        self.store.get_dms(user_id)
    }
}
