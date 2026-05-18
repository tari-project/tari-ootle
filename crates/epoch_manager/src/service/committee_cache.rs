//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, future::Future, sync::Arc};

use tari_ootle_common_types::{Epoch, NodeAddressable, ShardGroup, committee::Committee};
use tokio::sync::{OnceCell, RwLock};

type CacheMap<TAddr> = Arc<RwLock<HashMap<(Epoch, ShardGroup), Arc<OnceCell<Arc<Committee<TAddr>>>>>>>;

/// Shared cache of committees keyed by `(epoch, shard_group)`.
///
/// Committees are immutable within an epoch, so once populated for a given
/// `(epoch, shard_group)` the cached `Arc<Committee>` is shared by all callers
/// until [`CommitteeCache::clear`] is called at the next epoch boundary.
///
/// Concurrent first-fetches for the same key are coalesced via [`OnceCell`]:
/// only one initializer runs; the rest await the same future and all receive
/// the same `Arc`. After successful initialization, subsequent lookups are an
/// atomic load on the cell — no lock acquisition.
///
/// On error (or panic / cancellation of the initializer) the cell stays
/// uninitialized so the next caller retries — the right behaviour for
/// transient channel/RPC failures.
///
/// `Clone` is cheap: it clones the inner `Arc`. Hand a clone to the
/// [`EpochManagerHandle`] and keep one in the [`EpochManagerService`].
#[derive(Clone, Debug)]
pub struct CommitteeCache<TAddr> {
    inner: CacheMap<TAddr>,
}

impl<TAddr: NodeAddressable> CommitteeCache<TAddr> {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Returns the cached committee for `key`, or runs `init` to fetch it,
    /// coalescing with any concurrent caller for the same key.
    pub async fn get_or_try_init<F, Fut, E>(
        &self,
        key: (Epoch, ShardGroup),
        init: F,
    ) -> Result<Arc<Committee<TAddr>>, E>
    where
        F: FnOnce() -> Fut + Send,
        Fut: Future<Output = Result<Arc<Committee<TAddr>>, E>> + Send,
        E: Send,
    {
        // Brief write lock on the outer map: get-or-insert the per-key cell.
        // The lock is released before the init future is awaited.
        let cell = {
            let mut map = self.inner.write().await;
            map.entry(key).or_insert_with(|| Arc::new(OnceCell::new())).clone()
        };
        cell.get_or_try_init(init).await.cloned()
    }

    /// Drop all cached entries. Called on epoch advance, when committee
    /// assignments change for the new epoch.
    pub async fn clear(&self) {
        self.inner.write().await.clear();
    }
}

impl<TAddr: NodeAddressable> Default for CommitteeCache<TAddr> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };

    use tari_ootle_common_types::{Epoch, ShardGroup, committee::Committee};
    use tokio::time::sleep;

    use super::*;

    fn key() -> (Epoch, ShardGroup) {
        (Epoch(1), ShardGroup::new(1, 128))
    }

    #[tokio::test]
    async fn returns_cached_value_on_subsequent_calls() {
        let cache = CommitteeCache::<String>::new();
        let calls = AtomicUsize::new(0);

        let first = cache
            .get_or_try_init::<_, _, ()>(key(), || {
                calls.fetch_add(1, Ordering::SeqCst);
                async { Ok(Arc::new(Committee::empty())) }
            })
            .await
            .unwrap();

        let second = cache
            .get_or_try_init::<_, _, ()>(key(), || {
                calls.fetch_add(1, Ordering::SeqCst);
                async { Ok(Arc::new(Committee::empty())) }
            })
            .await
            .unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 1, "init must run only once");
        assert!(Arc::ptr_eq(&first, &second), "subsequent calls return the same Arc");
    }

    #[tokio::test]
    async fn coalesces_concurrent_first_fetches() {
        let cache = CommitteeCache::<String>::new();
        let calls = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::with_capacity(16);
        for _ in 0..16 {
            let cache = cache.clone();
            let calls = calls.clone();
            handles.push(tokio::spawn(async move {
                cache
                    .get_or_try_init::<_, _, ()>(key(), || {
                        calls.fetch_add(1, Ordering::SeqCst);
                        async {
                            // Force concurrent awaiters to pile up on the same OnceCell init.
                            sleep(Duration::from_millis(20)).await;
                            Ok(Arc::new(Committee::empty()))
                        }
                    })
                    .await
                    .unwrap()
            }));
        }

        let mut first = None;
        for r in handles {
            let r = r.await.unwrap();
            if first.is_none() {
                first = Some(r);
                continue;
            }
            assert!(Arc::ptr_eq(first.as_ref().unwrap(), &r), "all callers see the same Arc");
        }

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "init must run only once across awaiters"
        );
    }

    #[tokio::test]
    async fn error_does_not_poison_cell() {
        let cache = CommitteeCache::<String>::new();

        let err: Result<_, &'static str> = cache.get_or_try_init(key(), || async { Err("boom") }).await;
        assert_eq!(err, Err("boom"));

        // Next call retries because the cell stayed uninitialized.
        let ok = cache
            .get_or_try_init::<_, _, &'static str>(key(), || async { Ok(Arc::new(Committee::empty())) })
            .await;
        assert!(ok.is_ok());
    }

    #[tokio::test]
    async fn clear_drops_entries() {
        let cache = CommitteeCache::<String>::new();
        let calls = AtomicUsize::new(0);

        cache
            .get_or_try_init::<_, _, ()>(key(), || {
                calls.fetch_add(1, Ordering::SeqCst);
                async { Ok(Arc::new(Committee::empty())) }
            })
            .await
            .unwrap();

        cache.clear().await;

        cache
            .get_or_try_init::<_, _, ()>(key(), || {
                calls.fetch_add(1, Ordering::SeqCst);
                async { Ok(Arc::new(Committee::empty())) }
            })
            .await
            .unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 2, "init runs again after clear");
    }
}
