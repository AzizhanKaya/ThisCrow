use crate::id::id;
use crate::message::dispatch::Data;
use crate::message::snowflake::snowflake_id;
use crate::message::{Message, MessageType};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rocksdb::{ColumnFamilyDescriptor, DBWithThreadMode, MultiThreaded, Options};
use serde::{Deserialize, Serialize};
use std::path::Path;
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
    #[serde(default)]
    pub overwrited: Option<()>,
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
    db: RocksDB,
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

        Ok(Self { db })
    }

    pub fn write(&self, message: StoredMessage) -> Result<()> {
        let cf_messages = self.db.cf_handle(CF_MESSAGES).unwrap();

        let message_bytes = rmp_serde::to_vec_named(&message).expect("Failed to serialize message");

        /*
            Messages:
                message_id -> message
        */

        self.db
            .put_cf(&cf_messages, message.id.to_be_bytes(), &message_bytes)?;

        match message.group_id {
            Some(_) => self.index_channel_message(&message)?,
            None => self.index_dm_message(&message)?,
        }

        Ok(())
    }

    fn index_dm_message(&self, message: &StoredMessage) -> Result<()> {
        let cf_dm = self.db.cf_handle(CF_DM_INDEX).unwrap();
        let cf_user_dms = self.db.cf_handle(CF_USER_DMS).unwrap();

        let (user1, user2) = message.from.sort_pair(message.to);

        /*
            DM Index:
                user1 + user2 + message_id -> null
        */

        let mut index_key = [0u8; 16];
        index_key[0..4].copy_from_slice(&user1.to_be_bytes());
        index_key[4..8].copy_from_slice(&user2.to_be_bytes());
        index_key[8..16].copy_from_slice(&message.id.to_be_bytes());

        self.db.put_cf(&cf_dm, &index_key, &[])?;

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

        self.db
            .put_cf(&cf_user_dms, &key1, &message.id.to_be_bytes())?;
        self.db
            .put_cf(&cf_user_dms, &key2, &message.id.to_be_bytes())?;

        Ok(())
    }

    fn index_channel_message(&self, message: &StoredMessage) -> Result<()> {
        let cf_channel = self.db.cf_handle(CF_CHANNEL_INDEX).unwrap();

        let channel_id = message.to;

        /*
            Channel Index:
                channel_id + message_id -> null
        */

        let mut index_key = [0u8; 12];
        index_key[0..4].copy_from_slice(&channel_id.to_be_bytes());
        index_key[4..12].copy_from_slice(&message.id.to_be_bytes());

        self.db.put_cf(&cf_channel, &index_key, &[])?;

        Ok(())
    }

    fn box_to_i64(value: &[u8]) -> Option<i64> {
        if value.len() == 8 {
            let arr: [u8; 8] = value.try_into().ok()?;
            Some(i64::from_be_bytes(arr))
        } else {
            None
        }
    }

    fn datetime_to_min_snowflake(dt: &DateTime<Utc>) -> u64 {
        let snowflake: snowflake_id = (*dt).into();
        snowflake.0
    }

    fn datetime_to_max_snowflake(dt: &DateTime<Utc>) -> u64 {
        let snowflake: snowflake_id = (*dt).into();
        snowflake.0 | 0xFFFFF
    }

    pub fn get_direct_messages(
        &self,
        from: id,
        to: id,
        len: Option<i64>,
        start: Option<DateTime<Utc>>,
        end: DateTime<Utc>,
    ) -> Result<Vec<StoredMessage>> {
        let limit = len.unwrap_or(50) as usize;
        let cf_dm = self.db.cf_handle(CF_DM_INDEX).unwrap();
        let cf_messages = self.db.cf_handle(CF_MESSAGES).unwrap();

        let (user1, user2) = from.sort_pair(to);

        let mut prefix = [0u8; 8];
        prefix[0..4].copy_from_slice(&user1.to_be_bytes());
        prefix[4..8].copy_from_slice(&user2.to_be_bytes());

        let end_snowflake = Self::datetime_to_max_snowflake(&end);
        let start_snowflake = start
            .map(|s| Self::datetime_to_min_snowflake(&s))
            .unwrap_or(0);

        let mut seek_key = [0u8; 16];
        seek_key[0..8].copy_from_slice(&prefix);
        seek_key[8..16].copy_from_slice(&end_snowflake.to_be_bytes());

        let iter = self.db.iterator_cf(
            &cf_dm,
            rocksdb::IteratorMode::From(&seek_key, rocksdb::Direction::Reverse),
        );

        let mut messages = Vec::with_capacity(limit);

        for item in iter {
            let (key, _) = item?;

            if !key.starts_with(&prefix) {
                break;
            }

            if key.len() == 16 {
                let message_id_bytes: [u8; 8] = key[8..16].try_into().unwrap();
                let message_id_val = u64::from_be_bytes(message_id_bytes);

                if message_id_val < start_snowflake {
                    break;
                }

                if let Some(message_bytes) = self.db.get_cf(&cf_messages, message_id_bytes)? {
                    let stored: StoredMessage = rmp_serde::from_slice(&message_bytes)?;
                    messages.push(stored);
                }
            }

            if messages.len() >= limit {
                break;
            }
        }

        Ok(messages)
    }

    pub fn get_channel_messages(
        &self,
        channel_id: id,
        len: Option<i64>,
        start: Option<DateTime<Utc>>,
        end: DateTime<Utc>,
    ) -> Result<Vec<StoredMessage>> {
        let limit = len.unwrap_or(50) as usize;
        let cf_channel = self.db.cf_handle(CF_CHANNEL_INDEX).unwrap();
        let cf_messages = self.db.cf_handle(CF_MESSAGES).unwrap();

        let mut prefix = [0u8; 4];
        prefix[0..4].copy_from_slice(&channel_id.to_be_bytes());

        let end_snowflake = Self::datetime_to_max_snowflake(&end);
        let start_snowflake = start
            .map(|s| Self::datetime_to_min_snowflake(&s))
            .unwrap_or(0);

        let mut seek_key = [0u8; 12];
        seek_key[0..4].copy_from_slice(&prefix);
        seek_key[4..12].copy_from_slice(&end_snowflake.to_be_bytes());

        let iter = self.db.iterator_cf(
            &cf_channel,
            rocksdb::IteratorMode::From(&seek_key, rocksdb::Direction::Reverse),
        );

        let mut messages = Vec::with_capacity(limit);

        for item in iter {
            let (key, _) = item?;

            if !key.starts_with(&prefix) {
                break;
            }

            if key.len() == 12 {
                let message_id_bytes: [u8; 8] = key[4..12].try_into().unwrap();
                let message_id_val = u64::from_be_bytes(message_id_bytes);

                if message_id_val < start_snowflake {
                    break;
                }

                if let Some(message_bytes) = self.db.get_cf(&cf_messages, message_id_bytes)? {
                    let stored: StoredMessage = rmp_serde::from_slice(&message_bytes)?;
                    messages.push(stored);
                }
            }

            if messages.len() >= limit {
                break;
            }
        }

        Ok(messages)
    }

    pub fn get_dms(&self, user_id: id) -> Result<Vec<(id, snowflake_id)>> {
        let cf_user_dms = self.db.cf_handle(CF_USER_DMS).unwrap();

        let prefix = user_id.to_be_bytes();
        let iter = self.db.prefix_iterator_cf(&cf_user_dms, &prefix);

        let mut dm_users: Vec<(id, snowflake_id)> = Vec::new();

        for item in iter {
            let (key, value) = item?;

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
    }

    pub fn overwrite(&self, message: StoredMessage) -> Result<()> {
        let cf_messages = self.db.cf_handle(CF_MESSAGES).unwrap();

        let message_bytes = rmp_serde::to_vec(&message).expect("Failed to serialize message");

        self.db
            .put_cf(&cf_messages, message.id.to_be_bytes(), &message_bytes)?;

        Ok(())
    }

    pub fn delete(&self, message_id: snowflake_id) -> Result<()> {
        let cf_messages = self.db.cf_handle(CF_MESSAGES).unwrap();

        let message_bytes = self.db.get_cf(&cf_messages, message_id.to_be_bytes())?;

        if let Some(bytes) = message_bytes {
            let message: StoredMessage = rmp_serde::from_slice(&bytes)?;
            match message.group_id {
                Some(_) => {
                    let cf_channel = self.db.cf_handle(CF_CHANNEL_INDEX).unwrap();
                    let mut index_key = [0u8; 12];
                    index_key[0..4].copy_from_slice(&message.to.to_be_bytes());
                    index_key[4..12].copy_from_slice(&message.id.to_be_bytes());
                    self.db.delete_cf(&cf_channel, &index_key)?;
                }
                None => {
                    let cf_dm = self.db.cf_handle(CF_DM_INDEX).unwrap();
                    let (user1, user2) = message.from.sort_pair(message.to);

                    let mut index_key = [0u8; 16];
                    index_key[0..4].copy_from_slice(&user1.to_be_bytes());
                    index_key[4..8].copy_from_slice(&user2.to_be_bytes());
                    index_key[8..16].copy_from_slice(&message.id.to_be_bytes());
                    self.db.delete_cf(&cf_dm, &index_key)?;
                }
            }

            self.db.delete_cf(&cf_messages, message_id.to_be_bytes())?;
        }

        Ok(())
    }

    pub fn delete_dm(&self, from: id, to: id) -> Result<()> {
        let cf_user_dms = self.db.cf_handle(CF_USER_DMS).unwrap();

        let mut key = [0u8; 8];
        key[0..4].copy_from_slice(&from.to_be_bytes());
        key[4..8].copy_from_slice(&to.to_be_bytes());

        self.db.delete_cf(&cf_user_dms, &key)?;

        Ok(())
    }
}
