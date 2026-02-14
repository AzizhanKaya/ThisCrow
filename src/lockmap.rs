use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Arc, Mutex};
use tokio::sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};

pub struct LockMap<K: Hash + Eq + Clone> {
    map: Mutex<HashMap<K, Arc<RwLock<()>>>>,
}

impl<K: Hash + Eq + Clone> LockMap<K> {
    pub fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
        }
    }

    pub async fn read(&self, key: K) -> KeyReadGuard<'_, K> {
        let lock_arc = {
            let mut map = self.map.lock().unwrap();
            map.entry(key.clone())
                .or_insert_with(|| Arc::new(RwLock::new(())))
                .clone()
        };

        let guard = lock_arc.clone().read_owned().await;

        KeyReadGuard {
            map: self,
            key,
            _guard: guard,
            _arc: lock_arc,
        }
    }

    pub async fn write(&self, key: K) -> KeyWriteGuard<'_, K> {
        let lock_arc = {
            let mut map = self.map.lock().unwrap();
            map.entry(key.clone())
                .or_insert_with(|| Arc::new(RwLock::new(())))
                .clone()
        };

        let guard = lock_arc.clone().write_owned().await;

        KeyWriteGuard {
            map: self,
            key,
            _guard: guard,
            _arc: lock_arc,
        }
    }

    fn cleanup(&self, key: &K, lock_arc: &Arc<RwLock<()>>) {
        let mut map = self.map.lock().unwrap();

        if Arc::strong_count(lock_arc) <= 3 {
            map.remove(key);
        }
    }
}

pub struct KeyReadGuard<'a, K: Hash + Eq + Clone> {
    map: &'a LockMap<K>,
    key: K,
    _guard: OwnedRwLockReadGuard<()>,
    _arc: Arc<RwLock<()>>,
}

impl<'a, K: Hash + Eq + Clone> Drop for KeyReadGuard<'a, K> {
    fn drop(&mut self) {
        self.map.cleanup(&self.key, &self._arc);
    }
}

pub struct KeyWriteGuard<'a, K: Hash + Eq + Clone> {
    map: &'a LockMap<K>,
    key: K,
    _guard: OwnedRwLockWriteGuard<()>,
    _arc: Arc<RwLock<()>>,
}

impl<'a, K: Hash + Eq + Clone> Drop for KeyWriteGuard<'a, K> {
    fn drop(&mut self) {
        self.map.cleanup(&self.key, &self._arc);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::sleep;

    #[tokio::test]
    async fn arc_count_test() {
        let lock_map = Arc::new(LockMap::<i64>::new());

        let guard = lock_map.read(1).await;

        assert_eq!(Arc::strong_count(&guard._arc), 3);

        let guard = lock_map.read(1).await;

        assert_eq!(Arc::strong_count(&guard._arc), 5);
    }

    #[tokio::test]
    async fn test_lock_map() {
        let lock_map = Arc::new(LockMap::<String>::new());
        let key = "resource_1".to_string();
        let execution_order = Arc::new(Mutex::new(Vec::new()));

        let lock_map_clone = lock_map.clone();
        let exec_clone = execution_order.clone();
        let key_clone = key.clone();

        let writer1 = tokio::spawn(async move {
            let _guard = lock_map_clone.write(key_clone).await;
            exec_clone.lock().unwrap().push("writer1_start");
            sleep(Duration::from_millis(100)).await;
            exec_clone.lock().unwrap().push("writer1_end");
        });

        sleep(Duration::from_millis(20)).await;

        let lock_map_clone2 = lock_map.clone();
        let exec_clone2 = execution_order.clone();
        let key_clone2 = key.clone();

        let writer2 = tokio::spawn(async move {
            let _guard = lock_map_clone2.write(key_clone2).await;
            exec_clone2.lock().unwrap().push("writer2_start");
            exec_clone2.lock().unwrap().push("writer2_end");
        });

        let lock_map_clone3 = lock_map.clone();
        let key_diff = "resource_2".to_string();
        let reader_diff = tokio::spawn(async move {
            let _guard = lock_map_clone3.read(key_diff).await;
            sleep(Duration::from_millis(50)).await;
            "diff_key_ok"
        });

        let _ = tokio::join!(writer1, writer2, reader_diff);

        let exec = execution_order.lock().unwrap();
        assert_eq!(exec[0], "writer1_start");
        assert_eq!(exec[1], "writer1_end");
        assert_eq!(exec[2], "writer2_start");

        sleep(Duration::from_millis(50)).await;

        let map_inner = lock_map.map.lock().unwrap();
        assert!(
            map_inner.is_empty(),
            "Map temizlenmedi, içinde hala kilitler var: {:?}",
            map_inner.len()
        );
    }
}
