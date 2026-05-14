use crate::id::id;
use crate::message::snowflake::snowflake_id;
use crate::message::{Data, Message, MessageType};
use anyhow::{Context, Result};
use rocksdb::{ColumnFamilyDescriptor, DBWithThreadMode, MultiThreaded, Options};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

pub type RocksDB = DBWithThreadMode<MultiThreaded>;

const CF_MESSAGES: &str = "messages";
const CF_DM_INDEX: &str = "dm_index";
const CF_CHANNEL_INDEX: &str = "channel_index";
const CF_USER_DMS: &str = "user_dms";

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StoredMessage {
    pub id: snowflake_id,
    pub from: id,
    pub to: id,
    pub data: Data,
    pub group_id: Option<id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overwrited: Option<bool>,
}

impl From<StoredMessage> for Message<Data> {
    fn from(stored: StoredMessage) -> Self {
        Message {
            id: stored.id,
            from: stored.from,
            to: stored.to,
            data: stored.data,
            r#type: match stored.group_id {
                Some(gid) => MessageType::Group(gid),
                None => MessageType::Direct,
            },
        }
    }
}

impl<T: Serialize> TryFrom<Message<T>> for StoredMessage {
    type Error = anyhow::Error;

    fn try_from(message: Message<T>) -> Result<Self> {
        let buf =
            rmp_serde::to_vec_named(&message.data).context("Failed to serialize message data")?;

        let data: Data =
            rmp_serde::from_slice(&buf).context("Failed to convert byte slice to rmpv::Value")?;

        Ok(StoredMessage {
            id: message.id,
            from: message.from,
            to: message.to,
            data,
            group_id: match message.r#type {
                MessageType::Group(gid) | MessageType::InfoGroup(gid) => Some(gid),
                MessageType::Direct | MessageType::Info | MessageType::Server => None,
            },
            overwrited: None,
        })
    }
}

pub struct MessageStore {
    db: Arc<RocksDB>,
}

impl MessageStore {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        opts.set_max_open_files(1000);
        opts.set_keep_log_file_num(1);
        opts.set_max_total_wal_size(64 * 1024 * 1024);

        let cfs = vec![
            ColumnFamilyDescriptor::new(CF_MESSAGES, Options::default()),
            ColumnFamilyDescriptor::new(CF_DM_INDEX, Options::default()),
            ColumnFamilyDescriptor::new(CF_CHANNEL_INDEX, Options::default()),
            ColumnFamilyDescriptor::new(CF_USER_DMS, Options::default()),
        ];

        let db =
            RocksDB::open_cf_descriptors(&opts, path, cfs).context("Failed to open RocksDB")?;

        Ok(Self { db: Arc::new(db) })
    }

    pub async fn write(&self, message: StoredMessage) -> Result<()> {
        let db = self.db.clone();

        tokio::task::spawn_blocking(move || {
            let cf_messages = db
                .cf_handle(CF_MESSAGES)
                .context("CF_MESSAGES cf not found")?;

            /*
                Messages:
                    message_id -> message
            */

            let message_bytes =
                rmp_serde::to_vec_named(&message).context("Failed to serialize message")?;

            db.put_cf(&cf_messages, message.id.to_be_bytes(), &message_bytes)
                .context("Failed to put message in RocksDB")?;

            match message.group_id {
                Some(_) => Self::index_channel_message(&db, &message)?,
                None => Self::index_dm_message(&db, &message)?,
            }

            Ok(())
        })
        .await
        .context("Failed to spawn blocking task for write")?
    }

    pub async fn get(&self, message_id: snowflake_id) -> Result<StoredMessage> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let cf_messages = db
                .cf_handle(CF_MESSAGES)
                .context("CF_MESSAGES cf not found")?;

            let message_bytes = db
                .get_cf(&cf_messages, message_id.to_be_bytes())
                .context("Failed to get message from RocksDB")?;

            if let Some(bytes) = message_bytes {
                let message: StoredMessage =
                    rmp_serde::from_slice(&bytes).context("Failed to deserialize message")?;
                Ok(message)
            } else {
                anyhow::bail!("Message not found")
            }
        })
        .await
        .context("Failed to spawn blocking task for get")?
    }

    fn index_dm_message(db: &Arc<RocksDB>, message: &StoredMessage) -> Result<()> {
        let cf_dm = db
            .cf_handle(CF_DM_INDEX)
            .context("CF_DM_INDEX cf not found")?;
        let cf_user_dms = db
            .cf_handle(CF_USER_DMS)
            .context("CF_USER_DMS cf not found")?;

        let (user1, user2) = message.from.sort_pair(message.to);

        /*
            DM Index:
                user1 + user2 + message_id -> null
        */

        let mut index_key = [0u8; 16];
        index_key[0..4].copy_from_slice(&user1.to_be_bytes());
        index_key[4..8].copy_from_slice(&user2.to_be_bytes());
        index_key[8..16].copy_from_slice(&message.id.to_be_bytes());

        db.put_cf(&cf_dm, &index_key, &[])
            .context("Failed to put dm index")?;

        /*
            User DMs:
                user1 + user2 -> message_id
                user2 + user1 -> message_id
        */

        let mut key1 = [0u8; 8];
        key1[0..4].copy_from_slice(&user1.to_be_bytes());
        key1[4..8].copy_from_slice(&user2.to_be_bytes());

        let mut key2 = [0u8; 8];
        key2[0..4].copy_from_slice(&user2.to_be_bytes());
        key2[4..8].copy_from_slice(&user1.to_be_bytes());

        db.put_cf(&cf_user_dms, &key1, &message.id.to_be_bytes())
            .context("Failed to put user dms key1")?;
        db.put_cf(&cf_user_dms, &key2, &message.id.to_be_bytes())
            .context("Failed to put user dms key2")?;

        Ok(())
    }

    fn index_channel_message(db: &Arc<RocksDB>, message: &StoredMessage) -> Result<()> {
        let cf_channel = db
            .cf_handle(CF_CHANNEL_INDEX)
            .context("CF_CHANNEL_INDEX cf not found")?;

        let channel_id = message.to;

        /*
            Channel Index:
                channel_id + message_id -> null
        */

        let mut index_key = [0u8; 12];
        index_key[0..4].copy_from_slice(&channel_id.to_be_bytes());
        index_key[4..12].copy_from_slice(&message.id.to_be_bytes());

        db.put_cf(&cf_channel, &index_key, &[])
            .context("Failed to put channel index")?;

        Ok(())
    }

    pub async fn get_direct_messages(
        &self,
        from: id,
        to: id,
        len: Option<i64>,
        before: Option<snowflake_id>,
    ) -> Result<Vec<StoredMessage>> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let limit = len.unwrap_or(50) as usize;
            let cf_dm = db
                .cf_handle(CF_DM_INDEX)
                .context("CF_DM_INDEX cf not found")?;
            let cf_messages = db
                .cf_handle(CF_MESSAGES)
                .context("CF_MESSAGES cf not found")?;

            let (user1, user2) = from.sort_pair(to);

            let mut prefix = [0u8; 8];
            prefix[0..4].copy_from_slice(&user1.to_be_bytes());
            prefix[4..8].copy_from_slice(&user2.to_be_bytes());

            let before_id = before.map(|s| s.0).unwrap_or(u64::MAX);
            let seek_id = before_id.saturating_sub(1);

            let mut seek_key = [0u8; 16];
            seek_key[0..8].copy_from_slice(&prefix);
            seek_key[8..16].copy_from_slice(&seek_id.to_be_bytes());

            let iter = db.iterator_cf(
                &cf_dm,
                rocksdb::IteratorMode::From(&seek_key, rocksdb::Direction::Reverse),
            );

            let mut messages = Vec::with_capacity(limit);

            for item in iter {
                let (key, _) = item.context("Failed to get item from iterator")?;

                if !key.starts_with(&prefix) {
                    break;
                }

                if key.len() == 16 {
                    let message_id_bytes: [u8; 8] = key[8..16].try_into().unwrap();

                    if let Some(message_bytes) = db
                        .get_cf(&cf_messages, message_id_bytes)
                        .context("Failed to get message bytes")?
                    {
                        let stored: StoredMessage = rmp_serde::from_slice(&message_bytes)
                            .context("Failed to deserialize message")?;
                        messages.push(stored);
                    }
                }

                if messages.len() >= limit {
                    break;
                }
            }

            Ok(messages)
        })
        .await
        .context("Failed to spawn blocking task for get_direct_messages")?
    }

    pub async fn get_channel_messages(
        &self,
        channel_id: id,
        len: Option<i64>,
        before: Option<snowflake_id>,
    ) -> Result<Vec<StoredMessage>> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let limit = len.unwrap_or(50) as usize;
            let cf_channel = db
                .cf_handle(CF_CHANNEL_INDEX)
                .context("CF_CHANNEL_INDEX cf not found")?;
            let cf_messages = db
                .cf_handle(CF_MESSAGES)
                .context("CF_MESSAGES cf not found")?;

            let mut prefix = [0u8; 4];
            prefix[0..4].copy_from_slice(&channel_id.to_be_bytes());

            let before_id = before.map(|s| s.0).unwrap_or(u64::MAX);
            let seek_id = before_id.saturating_sub(1);

            let mut seek_key = [0u8; 12];
            seek_key[0..4].copy_from_slice(&prefix);
            seek_key[4..12].copy_from_slice(&seek_id.to_be_bytes());

            let iter = db.iterator_cf(
                &cf_channel,
                rocksdb::IteratorMode::From(&seek_key, rocksdb::Direction::Reverse),
            );

            let mut messages = Vec::with_capacity(limit);

            for item in iter {
                let (key, _) = item.context("Failed to get item from iterator")?;

                if !key.starts_with(&prefix) {
                    break;
                }

                if key.len() == 12 {
                    let message_id_bytes: [u8; 8] = key[4..12].try_into().unwrap();

                    if let Some(message_bytes) = db
                        .get_cf(&cf_messages, message_id_bytes)
                        .context("Failed to get message bytes")?
                    {
                        let stored: StoredMessage = rmp_serde::from_slice(&message_bytes)
                            .context("Failed to deserialize message")?;
                        messages.push(stored);
                    }
                }

                if messages.len() >= limit {
                    break;
                }
            }

            Ok(messages)
        })
        .await
        .context("Failed to spawn blocking task for get_channel_messages")?
    }

    pub async fn get_dms(&self, user_id: id) -> Result<Vec<(id, snowflake_id)>> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let cf_user_dms = db
                .cf_handle(CF_USER_DMS)
                .context("CF_USER_DMS cf not found")?;

            let prefix = user_id.to_be_bytes();
            let iter = db.prefix_iterator_cf(&cf_user_dms, &prefix);

            let mut dm_users: Vec<(id, snowflake_id)> = Vec::new();

            for item in iter {
                let (key, value) = item.context("Failed to get item from iterator")?;

                if !key.starts_with(&prefix) {
                    break;
                }

                if key.len() == 8 {
                    let other_id_bytes: [u8; 4] = key[4..8].try_into().unwrap();

                    let other_id_val = i32::from_be_bytes(other_id_bytes);

                    if value.len() == 8 {
                        let msg_id_bytes: [u8; 8] = value.as_ref().try_into().unwrap();
                        let message_id_val = u64::from_be_bytes(msg_id_bytes);

                        dm_users.push((id::from(other_id_val), snowflake_id(message_id_val)));
                    }
                }
            }

            Ok(dm_users)
        })
        .await
        .context("Failed to spawn blocking task for get_dms")?
    }

    pub async fn overwrite(&self, message: StoredMessage) -> Result<()> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let cf_messages = db
                .cf_handle(CF_MESSAGES)
                .context("CF_MESSAGES cf not found")?;

            let message_bytes =
                rmp_serde::to_vec_named(&message).context("Failed to serialize message")?;

            db.put_cf(&cf_messages, message.id.to_be_bytes(), &message_bytes)
                .context("Failed to put message in RocksDB")?;

            Ok(())
        })
        .await
        .context("Failed to spawn blocking task for overwrite")?
    }

    pub async fn delete(&self, message_id: snowflake_id) -> Result<()> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let cf_messages = db
                .cf_handle(CF_MESSAGES)
                .context("CF_MESSAGES cf not found")?;

            let message_bytes = db
                .get_cf(&cf_messages, message_id.to_be_bytes())
                .context("Failed to get message from RocksDB")?;

            if let Some(bytes) = message_bytes {
                let message: StoredMessage =
                    rmp_serde::from_slice(&bytes).context("Failed to deserialize message")?;
                match message.group_id {
                    Some(_) => {
                        let cf_channel = db
                            .cf_handle(CF_CHANNEL_INDEX)
                            .context("CF_CHANNEL_INDEX cf not found")?;
                        let mut index_key = [0u8; 12];
                        index_key[0..4].copy_from_slice(&message.to.to_be_bytes());
                        index_key[4..12].copy_from_slice(&message.id.to_be_bytes());
                        db.delete_cf(&cf_channel, &index_key)
                            .context("Failed to delete channel index")?;
                    }
                    None => {
                        let cf_dm = db
                            .cf_handle(CF_DM_INDEX)
                            .context("CF_DM_INDEX cf not found")?;
                        let (user1, user2) = message.from.sort_pair(message.to);

                        let mut index_key = [0u8; 16];
                        index_key[0..4].copy_from_slice(&user1.to_be_bytes());
                        index_key[4..8].copy_from_slice(&user2.to_be_bytes());
                        index_key[8..16].copy_from_slice(&message.id.to_be_bytes());
                        db.delete_cf(&cf_dm, &index_key)
                            .context("Failed to delete dm index")?;
                    }
                }

                db.delete_cf(&cf_messages, message_id.to_be_bytes())
                    .context("Failed to delete message from RocksDB")?;
            }

            Ok(())
        })
        .await
        .context("Failed to spawn blocking task for delete")?
    }

    pub async fn remove_dm(&self, from: id, to: id) -> Result<()> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let cf_user_dms = db
                .cf_handle(CF_USER_DMS)
                .context("CF_USER_DMS cf not found")?;

            let mut key = [0u8; 8];
            key[0..4].copy_from_slice(&from.to_be_bytes());
            key[4..8].copy_from_slice(&to.to_be_bytes());

            db.delete_cf(&cf_user_dms, &key)
                .context("Failed to delete user dms")?;

            Ok(())
        })
        .await
        .context("Failed to spawn blocking task for remove_dm")?
    }
}
