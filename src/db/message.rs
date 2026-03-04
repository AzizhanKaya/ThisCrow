use crate::id::id;
use crate::message::{Message, MessageType};
use crate::msgpack;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rmpv::Value;
use rocksdb::{ColumnFamilyDescriptor, DBWithThreadMode, MultiThreaded, Options};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub type RocksDB = DBWithThreadMode<MultiThreaded>;

const CF_MESSAGES: &str = "messages";
const CF_DM_INDEX: &str = "dm_index";
const CF_CHANNEL_INDEX: &str = "channel_index";
const CF_USER_DMS: &str = "user_dms";

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StoredMessage {
    pub id: id,
    pub from: id,
    pub to: id,
    pub data: Value,
    pub time: DateTime<Utc>,
    pub group_id: Option<id>,
}

impl<T: DeserializeOwned> From<StoredMessage> for Message<T> {
    fn from(stored: StoredMessage) -> Self {
        Message {
            id: stored.id,
            from: stored.from,
            to: stored.to,
            data: rmpv::ext::from_value(stored.data).unwrap(),
            time: stored.time,
            r#type: match stored.group_id {
                Some(gid) => MessageType::Group(gid),
                None => MessageType::Direct,
            },
        }
    }
}

impl<T: Serialize> From<Message<T>> for StoredMessage {
    fn from(message: Message<T>) -> Self {
        let buf = rmp_serde::to_vec_named(&message.data).expect("Message data serileştirilemedi");

        let data_value: rmpv::Value =
            rmp_serde::from_slice(&buf).expect("Byte dizisi rmpv::Value'ya dönüştürülemedi");

        StoredMessage {
            id: message.id,
            from: message.from,
            to: message.to,
            data: data_value,
            time: message.time,
            group_id: match message.r#type {
                MessageType::Group(gid) | MessageType::InfoGroup(gid) => Some(gid),
                MessageType::Direct | MessageType::Info | MessageType::Server => None,
            },
        }
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

    pub fn save_message(&self, message: StoredMessage) -> Result<()> {
        let msg_bytes = msgpack!(message);
        let cf_messages = self.db.cf_handle(CF_MESSAGES).unwrap();

        self.db
            .put_cf(&cf_messages, message.id.to_be_bytes(), &msg_bytes)?;

        match message.group_id {
            Some(_) => self.index_channel_message(&message)?,
            None => self.index_dm_message(&message)?,
        }

        Ok(())
    }

    fn index_dm_message(&self, msg: &StoredMessage) -> Result<()> {
        let cf_dm = self.db.cf_handle(CF_DM_INDEX).unwrap();
        let cf_user_dms = self.db.cf_handle(CF_USER_DMS).unwrap();

        let (user1, user2) = msg.from.sort_pair(msg.to);
        let reverse_ts = i64::MAX - msg.time.timestamp_millis();

        let mut index_key = [0u8; 32];
        index_key[0..8].copy_from_slice(&user1.to_be_bytes());
        index_key[8..16].copy_from_slice(&user2.to_be_bytes());
        index_key[16..24].copy_from_slice(&reverse_ts.to_be_bytes());
        index_key[24..32].copy_from_slice(&msg.id.to_be_bytes());

        self.db.put_cf(&cf_dm, &index_key, &[])?;

        let ts_bytes = msg.time.timestamp_millis().to_be_bytes();

        let mut key1 = [0u8; 16];
        key1[0..8].copy_from_slice(&msg.from.to_be_bytes());
        key1[8..16].copy_from_slice(&msg.to.to_be_bytes());

        let mut key2 = [0u8; 16];
        key2[0..8].copy_from_slice(&msg.to.to_be_bytes());
        key2[8..16].copy_from_slice(&msg.from.to_be_bytes());

        self.db.put_cf(&cf_user_dms, &key1, &ts_bytes)?;
        self.db.put_cf(&cf_user_dms, &key2, &ts_bytes)?;

        Ok(())
    }

    fn index_channel_message(&self, msg: &StoredMessage) -> Result<()> {
        let cf_channel = self.db.cf_handle(CF_CHANNEL_INDEX).unwrap();

        let group_id = msg.group_id.unwrap();
        let channel_id = msg.to;

        let reverse_ts = i64::MAX - msg.time.timestamp_millis();

        let mut index_key = [0u8; 32];
        index_key[0..8].copy_from_slice(&group_id.to_be_bytes());
        index_key[8..16].copy_from_slice(&channel_id.to_be_bytes());
        index_key[16..24].copy_from_slice(&reverse_ts.to_be_bytes());
        index_key[24..32].copy_from_slice(&msg.id.to_be_bytes());

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

    pub fn get_direct_messages(
        &self,
        from: id,
        to: id,
        len: Option<i64>,
        start: Option<DateTime<Utc>>,
        end: DateTime<Utc>,
    ) -> Result<Vec<Message<Value>>> {
        let limit = len.unwrap_or(50) as usize;
        let cf_dm = self.db.cf_handle(CF_DM_INDEX).unwrap();
        let cf_messages = self.db.cf_handle(CF_MESSAGES).unwrap();

        let (user1, user2) = if *from <= *to {
            (*from, *to)
        } else {
            (*to, *from)
        };

        let mut prefix = [0u8; 16];
        prefix[0..8].copy_from_slice(&user1.to_be_bytes());
        prefix[8..16].copy_from_slice(&user2.to_be_bytes());

        let end_reverse_ts = i64::MAX - end.timestamp_millis();
        let start_reverse_ts = start
            .map(|s| i64::MAX - s.timestamp_millis())
            .unwrap_or(i64::MAX);

        let mut range_start = [0u8; 24];
        range_start[0..16].copy_from_slice(&prefix);
        range_start[16..24].copy_from_slice(&end_reverse_ts.to_be_bytes());

        let mut range_end = [0u8; 24];
        range_end[0..16].copy_from_slice(&prefix);
        range_end[16..24].copy_from_slice(&start_reverse_ts.to_be_bytes());

        let mut messages = Vec::with_capacity(limit);
        let iter = self.db.iterator_cf(
            &cf_dm,
            rocksdb::IteratorMode::From(&range_start, rocksdb::Direction::Forward),
        );

        for item in iter {
            let (key, _value) = item?;

            if !key.starts_with(&prefix) {
                break;
            }

            if key.as_ref() >= range_end.as_slice() {
                break;
            }

            if key.len() == 32 {
                let msg_id_bytes: [u8; 8] = key[24..32].try_into().unwrap();
                if let Some(msg_bytes) = self.db.get_cf(&cf_messages, msg_id_bytes)? {
                    let stored: StoredMessage = rmp_serde::from_slice(&msg_bytes)?;
                    messages.push(stored.into());
                }
            }

            if messages.len() >= limit {
                break;
            }
        }

        messages.reverse();

        Ok(messages)
    }

    pub fn get_channel_messages(
        &self,
        group_id: id,
        channel_id: id,
        len: Option<i64>,
        start: Option<DateTime<Utc>>,
        end: DateTime<Utc>,
    ) -> Result<Vec<Message<Value>>> {
        let limit = len.unwrap_or(50) as usize;
        let cf_channel = self.db.cf_handle(CF_CHANNEL_INDEX).unwrap();
        let cf_messages = self.db.cf_handle(CF_MESSAGES).unwrap();

        let mut prefix = [0u8; 16];
        prefix[0..8].copy_from_slice(&group_id.to_be_bytes());
        prefix[8..16].copy_from_slice(&channel_id.to_be_bytes());

        let end_reverse_ts = i64::MAX - end.timestamp_millis();
        let start_reverse_ts = start
            .map(|s| i64::MAX - s.timestamp_millis())
            .unwrap_or(i64::MAX);

        let mut range_start = [0u8; 24];
        range_start[0..16].copy_from_slice(&prefix);
        range_start[16..24].copy_from_slice(&end_reverse_ts.to_be_bytes());

        let mut range_end = [0u8; 24];
        range_end[0..16].copy_from_slice(&prefix);
        range_end[16..24].copy_from_slice(&start_reverse_ts.to_be_bytes());

        let mut messages = Vec::with_capacity(limit);
        let iter = self.db.iterator_cf(
            &cf_channel,
            rocksdb::IteratorMode::From(&range_start, rocksdb::Direction::Forward),
        );

        for item in iter {
            let (key, _value) = item?;

            if !key.starts_with(&prefix) {
                break;
            }

            if key.as_ref() >= range_end.as_slice() {
                break;
            }

            if key.len() == 32 {
                let msg_id_bytes: [u8; 8] = key[24..32].try_into().unwrap();
                if let Some(msg_bytes) = self.db.get_cf(&cf_messages, msg_id_bytes)? {
                    let stored: StoredMessage = rmp_serde::from_slice(&msg_bytes)?;
                    messages.push(stored.into());
                }
            }

            if messages.len() >= limit {
                break;
            }
        }

        messages.reverse();

        Ok(messages)
    }

    pub fn get_dms(&self, user_id: id) -> Result<Vec<id>> {
        let cf_user_dms = self.db.cf_handle(CF_USER_DMS).unwrap();

        let prefix = user_id.to_be_bytes();
        let iter = self.db.prefix_iterator_cf(&cf_user_dms, &prefix);

        let mut temp_dm_users: Vec<(id, DateTime<Utc>)> = Vec::new();

        for item in iter {
            let (key, value) = item?;

            if !key.starts_with(&prefix) {
                break;
            }

            if key.len() == 16 {
                let other_id_bytes: [u8; 8] = key[8..16].try_into().unwrap();
                let other_id = u64::from_be_bytes(other_id_bytes);
                if let Some(ts_millis) = Self::box_to_i64(&value) {
                    if let Some(time) = DateTime::from_timestamp_millis(ts_millis) {
                        temp_dm_users.push((id(other_id), time));
                    }
                }
            }
        }

        temp_dm_users.sort_by(|a, b| a.1.cmp(&b.1));

        let dm_users: Vec<id> = temp_dm_users.into_iter().map(|(i, _)| i).collect();

        Ok(dm_users)
    }
}
