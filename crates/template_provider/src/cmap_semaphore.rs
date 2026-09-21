//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    fmt::{Debug, Formatter},
    hash::Hash,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

#[derive(Clone)]
pub struct ConcurrentMapSemaphore<K: Hash + Eq> {
    map: Arc<dashmap::DashMap<K, Arc<Mutex<()>>>>,
    global: Arc<std_semaphore::Semaphore>,
}

impl<K: Hash + Eq + Clone> ConcurrentMapSemaphore<K> {
    pub fn new(max_global_access: isize) -> Self {
        Self {
            map: Arc::new(dashmap::DashMap::new()),
            global: Arc::new(std_semaphore::Semaphore::new(max_global_access)),
        }
    }

    pub fn acquire(&self, key: K) -> ConcurrentMapSemaphoreGuard<'_, K> {
        let global_access = self.global.access();
        let map_mutex = self
            .map
            .entry(key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        ConcurrentMapSemaphoreGuard {
            _global_access: global_access,
            map: self.map.clone(),
            map_mutex,
            key,
        }
    }
}

impl<K: Hash + Eq> Debug for ConcurrentMapSemaphore<K> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CMapSemaphore")
            .field("map", &self.map.len())
            .field("global", &"...")
            .finish()
    }
}

pub struct ConcurrentMapSemaphoreGuard<'a, K: Hash + Eq> {
    /// The RAII handle to the global semaphore, which must be held for the duration of this guard
    _global_access: std_semaphore::SemaphoreGuard<'a>,
    map: Arc<dashmap::DashMap<K, Arc<Mutex<()>>>>,
    map_mutex: Arc<Mutex<()>>,
    key: K,
}

impl<K: Hash + Eq> ConcurrentMapSemaphoreGuard<'_, K> {
    pub fn access(&self) -> MutexGuard<'_, ()> {
        // The mutex guards `()`, so a panic under it leaves nothing half-written and the next
        // caller can take it. Propagating the poison instead would turn one panicking load into a
        // panic for every later caller of that key.
        self.map_mutex.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl<K: Hash + Eq> Drop for ConcurrentMapSemaphoreGuard<'_, K> {
    fn drop(&mut self) {
        // The entry must outlive every guard that took a reference to it, so that a thread arriving
        // later contends on the same mutex as the waiters already queued on it. Two references are
        // the map's own and this guard's; `remove_if` holds the shard lock across the count and the
        // removal, which is the same lock `acquire` takes to create an entry.
        self.map.remove_if(&self.key, |_, mutex| Arc::strong_count(mutex) == 2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A guard released while another still holds the key must leave the entry behind. Otherwise a
    /// thread arriving next creates a second mutex and enters the critical section alongside the
    /// guard already in it.
    #[test]
    fn an_entry_outlives_every_guard_but_the_last() {
        let sem = ConcurrentMapSemaphore::new(10);
        let first = sem.acquire(1);
        let second = sem.acquire(1);

        drop(first);

        let entry = sem.map.get(&1).expect("a guard still holds this key");
        assert!(
            Arc::ptr_eq(entry.value(), &second.map_mutex),
            "the surviving guard and the next arrival must contend on one mutex",
        );
        drop(entry);

        drop(second);
        assert!(sem.map.is_empty(), "the last guard leaves no entry behind");
    }

    /// Two keys are independent, so releasing one says nothing about the other.
    #[test]
    fn releasing_one_key_leaves_another_alone() {
        let sem = ConcurrentMapSemaphore::new(10);
        let first = sem.acquire(1);
        let second = sem.acquire(2);

        drop(first);

        assert!(!sem.map.contains_key(&1));
        assert!(sem.map.contains_key(&2));
        drop(second);
        assert!(sem.map.is_empty());
    }

    /// The critical section admits one holder at a time.
    #[test]
    fn one_key_admits_one_holder() {
        let sem = ConcurrentMapSemaphore::new(10);
        let guard = sem.acquire(1);
        let _access = guard.access();

        let other = sem.acquire(1);
        assert!(
            other.map_mutex.try_lock().is_err(),
            "a second holder must wait on the mutex the first took",
        );
    }
}
