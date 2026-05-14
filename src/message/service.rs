use crate::db::message::MessageStore;
use crate::db::message::StoredMessage;
use crate::id::id;
use crate::message::snowflake::snowflake_id;
use anyhow::Result;

pub struct MessageService {
    store: MessageStore,
}

impl MessageService {
    pub fn new(store: MessageStore) -> Self {
        Self { store }
    }

    pub async fn get(&self, message_id: snowflake_id) -> Result<StoredMessage> {
        self.store.get(message_id).await
    }

    pub async fn write(&self, msg: StoredMessage) -> Result<()> {
        self.store.write(msg).await
    }

    pub async fn overwrite(&self, message: StoredMessage) -> Result<()> {
        self.store.overwrite(message).await
    }

    pub async fn delete(&self, message_id: snowflake_id) -> Result<()> {
        self.store.delete(message_id).await
    }

    pub async fn remove_dm(&self, from: id, to: id) -> Result<()> {
        self.store.remove_dm(from, to).await
    }

    pub async fn get_channel_messages(
        &self,
        channel_id: id,
        before: Option<snowflake_id>,
        len: Option<i64>,
    ) -> Result<Vec<StoredMessage>> {
        self.store
            .get_channel_messages(channel_id, len, before)
            .await
    }

    pub async fn get_direct_messages(
        &self,
        user1: id,
        user2: id,
        before: Option<snowflake_id>,
        len: Option<i64>,
    ) -> Result<Vec<StoredMessage>> {
        self.store
            .get_direct_messages(user1, user2, len, before)
            .await
    }

    pub async fn get_dms(&self, user_id: id) -> Result<Vec<(id, snowflake_id)>> {
        self.store.get_dms(user_id).await
    }
}
