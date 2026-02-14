use crate::id::id;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sqlx::{FromRow, Row, postgres::PgRow};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Message<T> {
    pub id: id,
    #[serde(skip_deserializing, default)]
    pub from: id,
    pub to: id,
    pub data: T,
    pub time: DateTime<Utc>,
    #[serde(flatten)]
    pub r#type: MessageType,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, Copy)]
#[serde(rename_all = "lowercase", tag = "type", content = "server_id")]
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

impl<'r, T> FromRow<'r, PgRow> for Message<T>
where
    T: DeserializeOwned,
{
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let message_id: id = row.try_get("id")?;
        let from: id = row.try_get("from")?;
        let to: id = row.try_get("to")?;
        let time: chrono::DateTime<Utc> = row.try_get("time")?;

        let data_value: serde_json::Value = row.try_get("data")?;
        let data: T =
            serde_json::from_value(data_value).map_err(|e| sqlx::Error::ColumnDecode {
                index: "data".into(),
                source: Box::new(e),
            })?;

        let group_id: Option<id> = row.try_get("group_id")?;
        let r#type = match group_id {
            Some(gid) => MessageType::Group(gid),
            None => MessageType::Direct,
        };

        Ok(Self {
            id: message_id,
            from,
            to,
            data,
            time,
            r#type,
        })
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
