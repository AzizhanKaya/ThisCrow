use serde::{Deserialize, Deserializer, Serialize, de};

use crate::message::snowflake::snowflake_id;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileMeta {
    pub url: String,
    pub name: String,
    pub size: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde_with::skip_serializing_none]
pub struct MultiData {
    text: Option<String>,
    images: Option<Vec<String>>,
    videos: Option<Vec<String>>,
    files: Option<Vec<FileMeta>>,
}

impl<'de> Deserialize<'de> for MultiData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize, Debug)]
        struct Helper {
            text: Option<String>,
            images: Option<Vec<String>>,
            videos: Option<Vec<String>>,
            files: Option<Vec<FileMeta>>,
        }

        let helper = Helper::deserialize(deserializer)?;

        if helper.text.is_none()
            && helper.images.is_none()
            && helper.videos.is_none()
            && helper.files.is_none()
        {
            return Err(de::Error::custom("At least one field must be Some"));
        }
        Ok(MultiData {
            text: helper.text,
            images: helper.images,
            videos: helper.videos,
            files: helper.files,
        })
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum Data {
    Text(String),
    Encrypted {
        #[serde(with = "serde_bytes")]
        nonce: Vec<u8>,
        #[serde(with = "serde_bytes")]
        cipher: Vec<u8>,
    },
    MultiData(MultiData),
    Call {
        #[serde(deserialize_with = "require_option")]
        end_time: Option<f64>,
    },
    Reply {
        replied: snowflake_id,
        data: MultiData,
    },
}

fn require_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer)
}

impl Default for Data {
    fn default() -> Self {
        Data::Text("".to_string())
    }
}
