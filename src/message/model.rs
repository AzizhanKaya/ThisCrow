use crate::id::id;
use chrono::{DateTime, Utc};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Message<T> {
    pub id: id,
    #[serde(skip_deserializing, default)]
    pub from: id,
    pub to: id,
    pub data: T,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub time: DateTime<Utc>,
    #[serde(flatten)]
    pub r#type: MessageType,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, Copy)]
#[serde(rename_all = "lowercase", tag = "type", content = "group_id")]
pub enum MessageType {
    Direct,
    Group(id),
    #[default]
    Server,
    Info,
    InfoGroup(id),
}

impl<T> Message<T> {
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Message<U> {
        Message {
            id: self.id,
            from: self.from,
            to: self.to,
            time: self.time,
            r#type: self.r#type,
            data: f(self.data),
        }
    }
}

impl<T: Default> Default for Message<T> {
    fn default() -> Self {
        Self {
            id: Default::default(),
            from: Default::default(),
            to: Default::default(),
            data: Default::default(),
            time: Utc::now(),
            r#type: Default::default(),
        }
    }
}
