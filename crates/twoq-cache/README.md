# twoq-cache

TwoQ adaptive cache — scan-resistant hot/cold cache with promotion.

## Usage

```rust
use twoq_cache::TwoQCache;

let mut cache = TwoQCache::new(2, 4);
cache.insert("key", 42);
assert_eq!(cache.get(&"key"), Some(&42));
```

## How it works

- **Cold**: entries are inserted here first. Evicted if cold exceeds limit.
- **Hot**: entries promoted from cold when accessed. Protected from scan-style access patterns.
