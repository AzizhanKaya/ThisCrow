use crate::db::message::MessageStore;
use crate::db::message::StoredMessage;
use crate::id::id;
use crate::message::Message;
use anyhow::Result;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use rmpv::Value;
use std::collections::VecDeque;

const CACHE_SIZE: usize = 50;

struct Channel {
    messages: VecDeque<StoredMessage>,
    last_access: DateTime<Utc>,
}

impl Default for Channel {
    fn default() -> Self {
        Self {
            messages: VecDeque::with_capacity(CACHE_SIZE),
            last_access: Utc::now(),
        }
    }
}

impl Channel {
    fn add_cache(&mut self, msg: StoredMessage) {
        if self.messages.len() >= self.messages.capacity() {
            self.messages.pop_front();
        }
        self.messages.push_back(msg);
        self.last_access = Utc::now();
    }
}

struct Channels {
    channels: DashMap<id, Channel>,
}

pub struct MessageService {
    cache: Channels,
    store: MessageStore,
}

impl MessageService {
    pub fn new(store: MessageStore) -> Self {
        let cache = Channels {
            channels: DashMap::new(),
        };

        Self { cache, store }
    }

    pub fn write(&self, msg: StoredMessage) {
        if msg.group_id.is_some() {
            self.cache
                .channels
                .entry(msg.to)
                .or_default()
                .add_cache(msg.clone());
        }

        if let Err(e) = self.store.save_message(msg) {
            log::error!("Failed to save message to RocksDB: {}", e);
        }
    }

    pub fn get_channel_messages(
        &self,
        group_id: id,
        channel_id: id,
        start: Option<DateTime<Utc>>,
        end: DateTime<Utc>,
        len: Option<i64>,
    ) -> Result<Vec<Message<Value>>> {
        let limit = len.unwrap_or(50) as usize;

        let cached_messages = if let Some(mut entry) = self.cache.channels.get_mut(&channel_id) {
            entry.last_access = Utc::now();
            entry
                .messages
                .iter()
                .filter(|m| m.time <= end && start.map_or(true, |s| m.time >= s))
                .map(|m| m.clone().into())
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        if cached_messages.len() >= limit {
            return Ok(cached_messages.into_iter().take(limit).collect());
        }

        let needed = limit.saturating_sub(cached_messages.len());
        let db_end = cached_messages.first().map(|m| m.time).unwrap_or(end);

        let db_messages = self.store.get_channel_messages(
            group_id,
            channel_id,
            Some(needed as i64),
            start,
            db_end,
        )?;

        let messages: Vec<_> = db_messages.into_iter().chain(cached_messages).collect();

        Ok(messages)
    }

    pub fn get_direct_messages(
        &self,
        user1: id,
        user2: id,
        start: Option<DateTime<Utc>>,
        end: DateTime<Utc>,
        len: Option<i64>,
    ) -> anyhow::Result<Vec<Message<Value>>> {
        let limit = len.unwrap_or(50);

        let mut messages = self
            .store
            .get_direct_messages(user1, user2, Some(limit), start, end)?;

        messages.sort_by_key(|m| m.time);

        Ok(messages)
    }

    pub fn get_dms(&self, user_id: id) -> Result<Vec<id>> {
        self.store.get_dms(user_id)
    }
}
