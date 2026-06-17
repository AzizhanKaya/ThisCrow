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
    links: Option<Vec<String>>,
}

impl MultiData {
    pub fn has_attachment(&self) -> bool {
        self.images.is_some() || self.videos.is_some() || self.files.is_some()
    }

    pub fn has_links(&self) -> bool {
        self.text.is_some() && self.links.is_some()
    }
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
            links: Option<Vec<String>>,
        }

        impl Helper {
            fn has_attachment(&self) -> bool {
                self.images.is_some() || self.videos.is_some() || self.files.is_some()
            }

            fn has_links(&self) -> bool {
                self.text.is_some() && self.links.is_some()
            }
        }

        let helper = Helper::deserialize(deserializer)?;

        if !helper.has_attachment() && !helper.has_links() {
            return Err(de::Error::custom("At least one field must be Some"));
        }

        Ok(MultiData {
            text: helper.text,
            images: helper.images,
            videos: helper.videos,
            files: helper.files,
            links: helper.links,
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
