use serde::{Deserialize, Serialize};
use sqlx::postgres::PgTypeInfo;
use sqlx::{
    Postgres, Type, decode::Decode, encode::Encode, postgres::PgArgumentBuffer,
    postgres::PgValueRef,
};
use std::fmt;
use std::ops::Deref;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Hash, Default,
)]
#[serde(transparent)]
pub struct id(pub u64);

impl id {
    pub fn sort_pair(self, other: Self) -> (id, id) {
        if self <= other {
            (self, other)
        } else {
            (other, self)
        }
    }

    pub fn add(&mut self, n: u64) -> Self {
        self.0 += n;

        *self
    }
}

impl Type<Postgres> for id {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        <i64 as Type<Postgres>>::type_info()
    }
    fn compatible(ty: &PgTypeInfo) -> bool {
        <i64 as Type<Postgres>>::compatible(ty)
    }
}

impl Encode<'_, Postgres> for id {
    fn encode_by_ref(
        &self,
        buf: &mut PgArgumentBuffer,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        <i64 as Encode<Postgres>>::encode(self.0 as i64, buf)
    }
}

impl Decode<'_, Postgres> for id {
    fn decode(value: PgValueRef<'_>) -> Result<Self, sqlx::error::BoxDynError> {
        let val = <i64 as Decode<Postgres>>::decode(value)?;
        Ok(id(val as u64))
    }
}

impl Deref for id {
    type Target = u64;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<u64> for id {
    fn from(val: u64) -> Self {
        id(val)
    }
}

impl From<i64> for id {
    fn from(val: i64) -> Self {
        id(val as u64)
    }
}

impl From<id> for i64 {
    fn from(val: id) -> Self {
        val.0 as i64
    }
}

impl From<id> for u64 {
    fn from(val: id) -> Self {
        val.0 as u64
    }
}

impl fmt::Display for id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
