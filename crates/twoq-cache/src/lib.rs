use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

pub struct TwoQCache<K, V> {
    hot: HashMap<K, V>,
    hot_order: VecDeque<K>,
    cold: HashMap<K, V>,
    cold_order: VecDeque<K>,
    hot_limit: usize,
    cold_limit: usize,
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
        }
    }

    pub fn get(&mut self, key: &K) -> Option<&V> {
        if self.hot.contains_key(key) {
            if let Some(pos) = self.hot_order.iter().position(|k| k == key) {
                let k = self.hot_order.remove(pos).unwrap();
                self.hot_order.push_back(k);
            }
            return self.hot.get(key);
        }

        if let Some(v) = self.cold.remove(key) {
            if let Some(pos) = self.cold_order.iter().position(|k| k == key) {
                self.cold_order.remove(pos);
            }
            self.hot.insert(key.clone(), v);
            self.hot_order.push_back(key.clone());
            self.evict_hot();
            return self.hot.get(key);
        }

        None
    }

    pub fn insert(&mut self, key: K, value: V) {
        if self.hot.contains_key(&key) {
            self.hot.insert(key, value);
            return;
        }
        if self.cold.contains_key(&key) {
            self.cold.insert(key, value);
            return;
        }

        self.cold.insert(key.clone(), value);
        self.cold_order.push_back(key);
        self.evict_cold();
    }

    pub fn contains(&self, key: &K) -> bool {
        self.hot.contains_key(key) || self.cold.contains_key(key)
    }

    pub fn len(&self) -> usize {
        self.hot.len() + self.cold.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn hot_len(&self) -> usize {
        self.hot.len()
    }

    pub fn cold_len(&self) -> usize {
        self.cold.len()
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        if let Some(v) = self.hot.remove(key) {
            if let Some(pos) = self.hot_order.iter().position(|k| k == key) {
                self.hot_order.remove(pos);
            }
            return Some(v);
        }
        if let Some(v) = self.cold.remove(key) {
            if let Some(pos) = self.cold_order.iter().position(|k| k == key) {
                self.cold_order.remove(pos);
            }
            return Some(v);
        }
        None
    }

    fn evict_hot(&mut self) {
        while self.hot.len() > self.hot_limit {
            if let Some(oldest) = self.hot_order.pop_front() {
                if let Some(v) = self.hot.remove(&oldest) {
                    self.cold.insert(oldest.clone(), v);
                    self.cold_order.push_back(oldest);
                    self.evict_cold();
                }
            }
        }
    }

    fn evict_cold(&mut self) {
        while self.cold.len() > self.cold_limit {
            if let Some(oldest) = self.cold_order.pop_front() {
                self.cold.remove(&oldest);
            }
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
        let _ = cache.get(&"a"); // promote to hot
        cache.insert("b", 2);
        let _ = cache.get(&"b"); // promote to hot, evicts "a" to cold

        assert_eq!(cache.hot_len(), 1);
        assert!(cache.contains(&"a")); // still in cold
        assert!(cache.contains(&"b")); // in hot
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
    fn missing_get_returns_none() {
        let mut cache: TwoQCache<&str, i32> = TwoQCache::new(2, 2);
        assert_eq!(cache.get(&"missing"), None);
    }
}
