use crate::id::id;
use crate::message::snowflake::snowflake_id;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Message<T> {
    pub id: snowflake_id,
    #[serde(skip_deserializing, default)]
    pub from: id,
    pub to: id,
    pub data: T,
    #[serde(flatten)]
    pub r#type: MessageType,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, Copy)]
#[serde(rename_all = "snake_case", tag = "type", content = "group_id")]
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
            r#type: Default::default(),
        }
    }
}
