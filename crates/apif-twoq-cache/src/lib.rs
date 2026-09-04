#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

struct Entry<V> {
    value: V,
    tick: u64,
}

pub struct TwoQCache<K, V> {
    hot: HashMap<K, Entry<V>>,
    hot_order: VecDeque<(K, u64)>,
    cold: HashMap<K, Entry<V>>,
    cold_order: VecDeque<(K, u64)>,
    hot_limit: usize,
    cold_limit: usize,
    clock: u64,
}

impl<K: Hash + Eq + Clone, V> TwoQCache<K, V> {
    pub fn new(hot_limit: usize, cold_limit: usize) -> Self {
        Self {
            hot: HashMap::new(),
            hot_order: VecDeque::new(),
            cold: HashMap::new(),
            cold_order: VecDeque::new(),
            hot_limit,
            cold_limit,
            clock: 0,
        }
    }

    fn next_tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    fn compact(order: &mut VecDeque<(K, u64)>, map: &HashMap<K, Entry<V>>, limit: usize) {
        if order.len() <= (limit + 1).saturating_mul(4) {
            return;
        }
        order.retain(|(k, tick)| map.get(k).is_some_and(|e| e.tick == *tick));
    }

    pub fn get(&mut self, key: &K) -> Option<&V> {
        if self.hot.contains_key(key) {
            let tick = self.next_tick();
            if let Some(entry) = self.hot.get_mut(key) {
                entry.tick = tick;
            }
            self.hot_order.push_back((key.clone(), tick));
            Self::compact(&mut self.hot_order, &self.hot, self.hot_limit);
            return self.hot.get(key).map(|e| &e.value);
        }

        if let Some(entry) = self.cold.remove(key) {
            let tick = self.next_tick();
            self.hot.insert(
                key.clone(),
                Entry {
                    value: entry.value,
                    tick,
                },
            );
            self.hot_order.push_back((key.clone(), tick));
            self.evict_hot();
            return self.hot.get(key).map(|e| &e.value);
        }

        None
    }

    pub fn insert(&mut self, key: K, value: V) {
        let tick = self.next_tick();

        if let Some(slot) = self.hot.get_mut(&key) {
            slot.value = value;
            slot.tick = tick;
            self.hot_order.push_back((key, tick));
            Self::compact(&mut self.hot_order, &self.hot, self.hot_limit);
            return;
        }
        if let Some(slot) = self.cold.get_mut(&key) {
            slot.value = value;
            slot.tick = tick;
            self.cold_order.push_back((key, tick));
            Self::compact(&mut self.cold_order, &self.cold, self.cold_limit);
            return;
        }

        self.cold.insert(key.clone(), Entry { value, tick });
        self.cold_order.push_back((key, tick));
        self.evict_cold();
    }

    #[inline]
    #[must_use]
    pub fn contains(&self, key: &K) -> bool {
        self.hot.contains_key(key) || self.cold.contains_key(key)
    }

    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.hot.len() + self.cold.len()
    }

    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.hot.is_empty() && self.cold.is_empty()
    }

    #[inline]
    #[must_use]
    pub fn hot_len(&self) -> usize {
        self.hot.len()
    }

    #[inline]
    #[must_use]
    pub fn cold_len(&self) -> usize {
        self.cold.len()
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        if let Some(e) = self.hot.remove(key) {
            return Some(e.value);
        }
        self.cold.remove(key).map(|e| e.value)
    }

    fn pop_lru(order: &mut VecDeque<(K, u64)>, map: &HashMap<K, Entry<V>>) -> Option<K> {
        while let Some((key, tick)) = order.pop_front() {
            if map.get(&key).is_some_and(|e| e.tick == tick) {
                return Some(key);
            }
        }
        None
    }

    fn evict_hot(&mut self) {
        while self.hot.len() > self.hot_limit {
            let Some(oldest) = Self::pop_lru(&mut self.hot_order, &self.hot) else {
                break;
            };
            let Some(entry) = self.hot.remove(&oldest) else {
                break;
            };
            let tick = self.next_tick();
            self.cold.insert(
                oldest.clone(),
                Entry {
                    value: entry.value,
                    tick,
                },
            );
            self.cold_order.push_back((oldest, tick));
            self.evict_cold();
        }
    }

    fn evict_cold(&mut self) {
        while self.cold.len() > self.cold_limit {
            let Some(oldest) = Self::pop_lru(&mut self.cold_order, &self.cold) else {
                break;
            };
            self.cold.remove(&oldest);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_get() {
        let mut cache = TwoQCache::new(2, 4);
        cache.insert("a", 1);
        cache.insert("b", 2);
        assert_eq!(cache.get(&"a"), Some(&1));
        assert_eq!(cache.get(&"b"), Some(&2));
    }

    #[test]
    fn cold_to_hot_promotion() {
        let mut cache = TwoQCache::new(1, 3);
        cache.insert("a", 1);
        assert_eq!(cache.hot_len(), 0);
        assert_eq!(cache.cold_len(), 1);

        let _ = cache.get(&"a");
        assert_eq!(cache.hot_len(), 1);
        assert_eq!(cache.cold_len(), 0);
    }

    #[test]
    fn hot_eviction_to_cold() {
        let mut cache = TwoQCache::new(1, 2);
        cache.insert("a", 1);
        let _ = cache.get(&"a");
        cache.insert("b", 2);
        let _ = cache.get(&"b");

        assert_eq!(cache.hot_len(), 1);
        assert!(cache.contains(&"a"));
        assert!(cache.contains(&"b"));
    }

    #[test]
    fn cold_eviction_drops() {
        let mut cache = TwoQCache::new(2, 2);
        cache.insert("a", 1);
        cache.insert("b", 2);
        cache.insert("c", 3);
        cache.insert("d", 4);
        cache.insert("e", 5);

        assert!(!cache.contains(&"a"));
        assert!(!cache.contains(&"b"));
        assert!(!cache.contains(&"c"));
        assert!(cache.contains(&"d"));
        assert!(cache.contains(&"e"));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn scan_resistance() {
        let mut cache: TwoQCache<String, i32> = TwoQCache::new(2, 3);
        cache.insert("hot1".into(), 1);
        cache.insert("hot2".into(), 2);
        let _ = cache.get(&"hot1".to_string());
        let _ = cache.get(&"hot2".to_string());

        for i in 0..10 {
            cache.insert(format!("scan_{i}"), i);
        }

        assert!(cache.contains(&"hot1".to_string()));
        assert!(cache.contains(&"hot2".to_string()));
    }

    #[test]
    fn len_and_empty() {
        let mut cache: TwoQCache<&str, i32> = TwoQCache::new(2, 2);
        assert!(cache.is_empty());
        cache.insert("a", 1);
        assert_eq!(cache.len(), 1);
        assert!(!cache.is_empty());
    }

    #[test]
    fn remove_key() {
        let mut cache = TwoQCache::new(2, 2);
        cache.insert("a", 1);
        assert_eq!(cache.remove(&"a"), Some(1));
        assert!(!cache.contains(&"a"));
        assert_eq!(cache.remove(&"a"), None);
    }

    #[test]
    fn update_existing_key() {
        let mut cache = TwoQCache::new(2, 2);
        cache.insert("a", 1);
        cache.insert("a", 42);
        assert_eq!(cache.get(&"a"), Some(&42));
    }

    #[test]
    fn update_refreshes_hot_recency() {
        let mut cache = TwoQCache::new(2, 1);
        cache.insert("a", 1);
        let _ = cache.get(&"a");
        cache.insert("b", 2);
        let _ = cache.get(&"b");

        cache.insert("a", 10);

        cache.insert("c", 3);
        let _ = cache.get(&"c");
        cache.insert("d", 4);
        let _ = cache.get(&"d");

        assert!(cache.contains(&"a"), "refreshed key must survive");
        assert!(!cache.contains(&"b"), "stale key must be evicted");
        assert_eq!(cache.get(&"a"), Some(&10), "value must be updated");
    }

    #[test]
    fn missing_get_returns_none() {
        let mut cache: TwoQCache<&str, i32> = TwoQCache::new(2, 2);
        assert_eq!(cache.get(&"missing"), None);
    }

    #[test]
    fn repeated_hits_do_not_grow_the_queue_without_bound() {
        let mut cache: TwoQCache<String, i32> = TwoQCache::new(4, 4);
        cache.insert("k".into(), 1);
        let _ = cache.get(&"k".to_string());
        for _ in 0..10_000 {
            let _ = cache.get(&"k".to_string());
        }
        assert!(
            cache.hot_order.len() <= 4 * (4 + 1) + 1,
            "queue grew to {}",
            cache.hot_order.len()
        );
        assert_eq!(cache.get(&"k".to_string()), Some(&1));
    }

    #[test]
    fn removed_keys_do_not_block_eviction() {
        let mut cache: TwoQCache<String, i32> = TwoQCache::new(1, 1);
        cache.insert("a".into(), 1);
        let _ = cache.get(&"a".to_string());
        assert_eq!(cache.remove(&"a".to_string()), Some(1));

        for i in 0..5 {
            cache.insert(format!("k{i}"), i);
            let _ = cache.get(&format!("k{i}"));
        }
        assert!(!cache.contains(&"a".to_string()));
        assert!(cache.len() <= 2);
    }
}
