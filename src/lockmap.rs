use async_lock::{RwLock, RwLockReadGuardArc, RwLockWriteGuardArc};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;

pub struct LockMap<K: Hash + Eq + Clone> {
    map: Mutex<HashMap<K, Arc<RwLock<()>>>>,
}

impl<K: Hash + Eq + Clone + Debug> LockMap<K> {
    pub fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
        }
    }

    pub async fn read(&self, key: K) -> KeyGuard<'_, K> {
        let lock_arc = {
            let mut map = self.map.lock();
            map.entry(key.clone())
                .or_insert_with(|| Arc::new(RwLock::new(())))
                .clone()
        };

        let guard = lock_arc.read_arc().await;

        KeyGuard {
            map: self,
            key,
            guard: Some(GuardType::Read(guard)),
        }
    }

    pub async fn write(&self, key: K) -> KeyGuard<'_, K> {
        let lock_arc = {
            let mut map = self.map.lock();
            map.entry(key.clone())
                .or_insert_with(|| Arc::new(RwLock::new(())))
                .clone()
        };

        let guard = lock_arc.write_arc().await;

        KeyGuard {
            map: self,
            key,
            guard: Some(GuardType::Write(guard)),
        }
    }

    fn cleanup(&self, key: &K) {
        let mut map = self.map.lock();

        if let Some(arc) = map.get(key) {
            if Arc::strong_count(arc) == 1 {
                map.remove(key);
            }
        }
    }
}

enum GuardType {
    Read(RwLockReadGuardArc<()>),
    Write(RwLockWriteGuardArc<()>),
}

pub struct KeyGuard<'a, K: Hash + Eq + Clone + Debug> {
    map: &'a LockMap<K>,
    key: K,
    guard: Option<GuardType>,
}

impl<'a, K: Hash + Eq + Clone + Debug> Drop for KeyGuard<'a, K> {
    fn drop(&mut self) {
        drop(self.guard.take());
        self.map.cleanup(&self.key);
    }
}
