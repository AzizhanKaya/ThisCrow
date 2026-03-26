use anyhow::anyhow;
use chrono::{DateTime, TimeZone, Utc};
use derive_more::{Deref, Display, From, Into};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};
use std::u8;

const EPOCH: u64 = 1767225600000;

const SEQUENCE_BITS: u8 = 12;
const NODE_BITS: u8 = 8;

const MAX_SEQUENCE: u64 = (1 << SEQUENCE_BITS) - 1;

const NODE_SHIFT: u8 = SEQUENCE_BITS;
const TIMESTAMP_SHIFT: u8 = SEQUENCE_BITS + NODE_BITS;

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    Serialize,
    Deserialize,
    Display,
    From,
    Into,
    Deref,
)]
#[serde(transparent)]
pub struct snowflake_id(pub u64);

impl TryFrom<snowflake_id> for DateTime<Utc> {
    type Error = anyhow::Error;

    fn try_from(value: snowflake_id) -> Result<Self, Self::Error> {
        let raw_ts = value.0 >> TIMESTAMP_SHIFT;

        let actual_ts = raw_ts + EPOCH;

        Utc.timestamp_millis_opt(actual_ts as i64)
            .single()
            .ok_or_else(|| anyhow!("Invalid or ambiguous snowflake timestamp: {}", value.0))
    }
}

impl From<DateTime<Utc>> for snowflake_id {
    fn from(value: DateTime<Utc>) -> Self {
        let absolute_ts = value.timestamp_millis() as u64;
        let relative_ts = absolute_ts.saturating_sub(EPOCH);

        snowflake_id(relative_ts << TIMESTAMP_SHIFT)
    }
}

pub struct SnowflakeGenerator {
    node_id: u8,
    counter: AtomicU64,
}

impl SnowflakeGenerator {
    pub fn new(node_id: u8) -> Self {
        Self {
            node_id,
            counter: AtomicU64::new(0),
        }
    }

    pub fn generate(&self) -> snowflake_id {
        let mut prev = self.counter.load(Ordering::Relaxed);

        loop {
            let now = self.current_millis();
            let last_ts = prev >> SEQUENCE_BITS;
            let last_seq = prev & MAX_SEQUENCE;

            let (next_ts, next_seq, wait) = if now <= last_ts {
                if last_seq == MAX_SEQUENCE {
                    (last_ts + 1, 0, true)
                } else {
                    (last_ts, last_seq + 1, false)
                }
            } else {
                (now, 0, false)
            };

            if wait {
                let mut current_now = self.current_millis();
                while current_now <= last_ts {
                    std::hint::spin_loop();
                    current_now = self.current_millis();
                }
                prev = self.counter.load(Ordering::Relaxed);
                continue;
            }

            let next = (next_ts << SEQUENCE_BITS) | next_seq;

            match self.counter.compare_exchange_weak(
                prev,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return snowflake_id(
                        (next_ts << TIMESTAMP_SHIFT)
                            | ((self.node_id as u64) << NODE_SHIFT)
                            | next_seq,
                    );
                }
                Err(current) => {
                    prev = current;
                }
            }
        }
    }

    #[inline]
    fn current_millis(&self) -> u64 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH + Duration::from_millis(EPOCH))
            .expect("Time error")
            .as_millis() as u64
    }
}
