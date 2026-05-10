use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

const DEFAULT_CALLBACK_TTL: Duration = Duration::from_secs(15 * 60);
const DEFAULT_MAX_CALLBACKS: usize = 1024;

#[derive(Clone, Debug)]
pub struct ShortCallbackStore<T> {
    next_id: u64,
    entries: BTreeMap<String, ShortCallbackEntry<T>>,
    ttl: Duration,
    max_entries: usize,
}

#[derive(Clone, Debug)]
struct ShortCallbackEntry<T> {
    value: T,
    created_at: Instant,
}

impl<T> ShortCallbackStore<T> {
    pub fn with_limits(ttl: Duration, max_entries: usize) -> Self {
        Self {
            next_id: 0,
            entries: BTreeMap::new(),
            ttl,
            max_entries,
        }
    }

    pub fn reserve_key(&mut self) -> String {
        self.sweep_expired();
        self.next_id += 1;
        base36(self.next_id)
    }

    pub fn insert(&mut self, value: T) -> String {
        let key = self.reserve_key();
        self.insert_reserved(key.clone(), value);
        key
    }

    pub fn insert_reserved(&mut self, key: String, value: T) {
        self.sweep_expired();
        self.entries.insert(
            key,
            ShortCallbackEntry {
                value,
                created_at: Instant::now(),
            },
        );
        self.sweep_overflow();
    }

    pub fn remove(&mut self, key: &str) -> Option<T> {
        self.sweep_expired();
        self.entries.remove(key).map(|entry| entry.value)
    }

    fn sweep_expired(&mut self) {
        let ttl = self.ttl;
        self.entries
            .retain(|_, entry| entry.created_at.elapsed() <= ttl);
    }

    fn sweep_overflow(&mut self) {
        while self.entries.len() > self.max_entries {
            let Some(key) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.created_at)
                .map(|(key, _)| key.clone())
            else {
                return;
            };
            self.entries.remove(&key);
        }
    }
}

impl<T> Default for ShortCallbackStore<T> {
    fn default() -> Self {
        Self {
            next_id: 0,
            entries: BTreeMap::new(),
            ttl: DEFAULT_CALLBACK_TTL,
            max_entries: DEFAULT_MAX_CALLBACKS,
        }
    }
}

fn base36(mut value: u64) -> String {
    if value == 0 {
        return "0".into();
    }
    let mut chars = Vec::new();
    while value > 0 {
        let digit = (value % 36) as u8;
        chars.push(match digit {
            0..=9 => (b'0' + digit) as char,
            _ => (b'a' + digit - 10) as char,
        });
        value /= 36;
    }
    chars.into_iter().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::ShortCallbackStore;
    use std::time::Duration;

    #[test]
    fn short_callback_store_consumes_reserved_key_once() {
        let mut store = ShortCallbackStore::default();
        let key = store.reserve_key();
        store.insert_reserved(key.clone(), "value");

        assert_eq!(store.remove(&key), Some("value"));
        assert_eq!(store.remove(&key), None);
    }

    #[test]
    fn short_callback_store_drops_oldest_entry_over_capacity() {
        let mut store = ShortCallbackStore::with_limits(Duration::from_secs(60), 1);
        let first = store.insert("first");
        let second = store.insert("second");

        assert_eq!(store.remove(&first), None);
        assert_eq!(store.remove(&second), Some("second"));
    }
}
