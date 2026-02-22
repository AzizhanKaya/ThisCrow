use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};
use std::u8;
use tokio::time::sleep;

const EPOCH: u64 = 1767225600000;

const SEQUENCE_BITS: u8 = 12;
const NODE_BITS: u8 = 8;

const MAX_SEQUENCE: u64 = (1 << SEQUENCE_BITS) - 1;

const NODE_SHIFT: u8 = SEQUENCE_BITS;
const TIMESTAMP_SHIFT: u8 = SEQUENCE_BITS + NODE_BITS;

pub struct Snowflake {
    node_id: u8,
    counter: AtomicU64,
}

impl Snowflake {
    pub fn new(node_id: u8) -> Self {
        Self {
            node_id,
            counter: AtomicU64::new(0),
        }
    }

    pub async fn generate(&self) -> u64 {
        loop {
            let now = self.current_millis();

            let prev = self.counter.load(Ordering::Relaxed);

            let last_ts = prev >> SEQUENCE_BITS;
            let last_seq = prev & MAX_SEQUENCE;

            let (next_ts, next_seq, wait) = if now < last_ts {
                if last_seq == MAX_SEQUENCE {
                    (last_ts + 1, 0, true)
                } else {
                    (last_ts, last_seq + 1, false)
                }
            } else if now == last_ts {
                if last_seq == MAX_SEQUENCE {
                    (last_ts + 1, 0, true)
                } else {
                    (last_ts, last_seq + 1, false)
                }
            } else {
                (now, 0, false)
            };

            if wait {
                let delay = next_ts.saturating_sub(now);
                sleep(Duration::from_millis(delay)).await;
                continue;
            }

            let next = (next_ts << SEQUENCE_BITS) | next_seq;

            if self
                .counter
                .compare_exchange(prev, next, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return (next_ts << TIMESTAMP_SHIFT)
                    | ((self.node_id as u64) << NODE_SHIFT)
                    | next_seq;
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
