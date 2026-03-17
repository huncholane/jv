use std::hash::{Hash, Hasher};

pub struct CacheGuard<T> {
    key: u64,
    value: T,
}

impl<T> CacheGuard<T> {
    pub fn new(initial: T) -> Self {
        Self { key: 0, value: initial }
    }

    /// Returns `Some(&mut T)` if the key changed (cache is dirty).
    /// Returns `None` if cache is still valid.
    pub fn get_if_changed(&mut self, new_key: u64) -> Option<&mut T> {
        if new_key != self.key {
            self.key = new_key;
            Some(&mut self.value)
        } else {
            None
        }
    }

    /// Force the cache to be dirty on next check.
    pub fn invalidate(&mut self) {
        self.key = 0;
    }

    pub fn value(&self) -> &T {
        &self.value
    }

    pub fn value_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

/// Compute a hash key from a closure that hashes into a DefaultHasher.
pub fn hash_key(f: impl FnOnce(&mut std::collections::hash_map::DefaultHasher)) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    f(&mut h);
    h.finish()
}

/// Convenience: hash a single hashable value.
pub fn hash_one<T: Hash>(val: &T) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    val.hash(&mut h);
    h.finish()
}
