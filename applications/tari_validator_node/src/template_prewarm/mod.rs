//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Background compilation of templates a node is about to need.
//!
//! Compiling a WASM template is unpriced work on the critical path: a validator that reaches a
//! transaction calling a template this process has not seen pays its Cranelift compile inside
//! consensus execution, and the leader pays it inside the proposal path. This subsystem moves that
//! compile earlier, to the moment the node learns it will need the template.
//!
//! Three properties hold it in place:
//!
//! - The cache is a memo, never an authority. A prewarm that has not finished, or never ran, is a cache miss, and
//!   `get_template` compiles inline exactly as it does today. Nothing here can change the outcome of an execution, only
//!   when it happens.
//! - Only validated transactions are prewarmed. Compiling for anything that reached the node unvalidated is free CPU
//!   for whoever can gossip.
//! - A wait is a delay, never a cancellation. [`PrewarmWait::wait`] gives up on its timeout while the compile it was
//!   waiting on keeps running, so the executor that follows joins that work rather than starting its own.
//!
//! Work goes through the *shared* [`MemoryCacheTemplateProvider`], not a private copy: its
//! per-address semaphore is then what coalesces a prewarm with an execution that wants the same
//! template, so the two never compile it twice and no single-flight machinery is needed here.
//!
//! This is validator-only. The indexer has no mempool and stores no substates, so neither trigger
//! exists there; its dry-run executor compiles on demand under the bounded cache.
//!
//! [`MemoryCacheTemplateProvider`]: tari_ootle_template_provider::MemoryCacheTemplateProvider

use std::{
    collections::{HashMap, HashSet, hash_map::Entry},
    fmt,
    sync::{
        Arc,
        Mutex,
        MutexGuard,
        PoisonError,
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

use log::*;
use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::services::template_provider::TemplateProvider;
use tari_ootle_storage::{StateStore, StateStoreReadTransaction};
use tari_ootle_template_provider::ResidentTemplateProvider;
use tari_ootle_transaction::{ComponentReference, Instruction, Transaction};
use tari_template_builtin::is_builtin_template_address;
use tari_template_lib::types::{ComponentAddress, TemplateAddress};
use tokio::sync::oneshot;

#[cfg(feature = "metrics")]
use crate::template_prewarm::metrics::PrometheusPrewarmMetrics;

mod hooks;
pub use hooks::TemplatePrewarmHooks;

#[cfg(feature = "metrics")]
mod metrics;

const LOG_TARGET: &str = "tari::validator_node::template_prewarm";

/// Templates the queue holds before [`TemplatePrewarmer::enqueue`] starts dropping them.
///
/// A drop costs the latency this subsystem exists to remove and nothing else, so the queue is sized
/// to absorb a burst of distinct templates rather than to never overflow. Duplicates do not occupy
/// it: a template already queued or in flight collects another waiter instead of another slot.
const QUEUE_CAPACITY: usize = 2048;

/// Upper bound on worker threads, whatever the machine's core count.
///
/// The pool's thread count is its whole concurrency budget. Each worker in a compile holds one of
/// the shared provider's `CONCURRENT_ACCESS_LIMIT` (100) permits, so the bound must stay a small
/// fraction of that limit for executor lookups to keep finding permits free.
const MAX_WORKERS: usize = 4;

/// Everyone waiting on each template that is queued or in flight.
type Waiters = HashMap<TemplateAddress, Vec<oneshot::Sender<()>>>;

/// An address leaves [`Waiters`] only when its compile has finished, which is what both
/// deduplicates the queue and releases the waits.
type Queued = Arc<Mutex<Waiters>>;

/// Cloneable handle to the prewarm pool. Enqueueing never blocks.
#[derive(Clone)]
pub struct TemplatePrewarmer {
    tx: SyncSender<TemplateAddress>,
    queued: Queued,
    residency: Arc<dyn ResidentTemplateProvider + Send + Sync>,
    components: Arc<dyn ComponentTemplateLookup>,
    #[cfg(feature = "metrics")]
    metrics: PrometheusPrewarmMetrics,
}

impl TemplatePrewarmer {
    /// Queue the templates a validated transaction needs and does not already have compiled, and
    /// return a handle that completes once each of those compiles has.
    ///
    /// The caller must have validated the transaction first, and should only prewarm one it is
    /// involved in.
    pub fn prewarm_transaction(&self, transaction: &Transaction) -> PrewarmWait {
        let instructions = || transaction.instructions().iter().chain(transaction.fee_instructions());

        // A `CallMethod` names a component rather than a template, so every component the
        // transaction calls into is resolved up front, over one snapshot. Doing it here rather than
        // on a worker is what makes the returned wait exact: the caller blocks on the compiles it
        // needs and on nothing else.
        let called: Vec<_> = instructions().filter_map(called_component).collect();
        let instantiated = self.components.templates_of(&called);

        // Deduplicated before enqueueing, so that a template named twice by one transaction is one
        // compile to wait for rather than two waiters on the same one.
        let mut seen = HashSet::new();
        let receivers = instructions()
            .filter_map(|instruction| template_for(instruction, &instantiated))
            .filter(|address| seen.insert(*address))
            .filter_map(|address| self.enqueue(address))
            .collect();
        PrewarmWait { receivers }
    }

    /// Queue a template without waiting for it.
    pub fn prewarm_template(&self, address: TemplateAddress) {
        let _ignore = self.enqueue(address);
    }

    /// Queue `address` unless it is already compiled, and return what completes when its compile
    /// does. `None` means there is nothing to wait for: the template is resident, or the queue was
    /// full and this request was dropped.
    fn enqueue(&self, address: TemplateAddress) -> Option<oneshot::Receiver<()>> {
        // Builtins are compiled at startup and held for the life of the provider.
        if is_builtin_template_address(&address) || self.residency.is_resident(&address) {
            return None;
        }

        let (tx, rx) = oneshot::channel();
        {
            let mut queued = self.queued();
            match queued.entry(address) {
                Entry::Occupied(mut waiters) => {
                    waiters.get_mut().push(tx);
                    return Some(rx);
                },
                Entry::Vacant(slot) => {
                    slot.insert(vec![tx]);
                },
            }
        }

        if self.tx.try_send(address).is_err() {
            // Dropping the waiters releases everyone blocked on this address, which is what a full
            // queue degrades to: the compile happens during execution instead.
            self.queued().remove(&address);
            debug!(target: LOG_TARGET, "Prewarm queue is full, dropping template {address}");
            #[cfg(feature = "metrics")]
            self.metrics.on_dropped();
            return None;
        }

        #[cfg(feature = "metrics")]
        self.metrics.on_enqueued(self.queued().len());
        Some(rx)
    }

    fn queued(&self) -> MutexGuard<'_, Waiters> {
        self.queued.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl fmt::Debug for TemplatePrewarmer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TemplatePrewarmer")
            .field("queued", &self.queued().len())
            .finish_non_exhaustive()
    }
}

/// Allowance per cold template in [`PrewarmWait::timeout`].
///
/// About twice the ~260 ms a 1 MiB binary — the largest publishable, per
/// `EngineLimits::max_template_binary_size_bytes` — takes to compile, measured at 52 ms for 151 KiB
/// and 142 ms for 530 KiB. The doubling is what absorbs a loaded machine.
///
/// It scales linearly in the number of templates although `N` workers finish `K` of them in about
/// `ceil(K / N)` compiles' time. The generosity is deliberate: this is a starvation valve, and
/// dividing by the worker count would tighten it towards the point where a busy pool starts handing
/// transactions over cold.
///
/// The unit is templates rather than bytes even though compile time tracks size closely
/// (~0.25 ms/KiB + 16 ms). A size is known only for a binary the transaction carries; for a
/// `CallFunction` or `CallMethod` it takes fetching the template to learn, which is the work being
/// waited on.
const PREWARM_WAIT_PER_TEMPLATE: Duration = Duration::from_millis(512);

/// Bound on [`PrewarmWait::timeout`] however many templates a transaction needs.
///
/// Nothing caps the distinct templates one transaction may call, so this is the real bound and the
/// per-template rate only shapes the ramp up to it. Reaching it hands the transaction over cold,
/// which is where a node without a prewarm pool starts: a degradation, not a stall.
///
/// What a waiting transaction costs is its own admission delay. Its gossip verdict is reported and,
/// if it is local, it is propagated before the wait begins, and the caller waits on a task of its
/// own — the mempool service is a single task and would otherwise pay this bound on behalf of every
/// message queued behind it.
const PREWARM_WAIT_CEILING: Duration = Duration::from_secs(2);

/// What a caller holds while the compiles it asked for are in flight.
#[must_use = "a prewarm that is never waited on is fire-and-forget"]
pub struct PrewarmWait {
    receivers: Vec<oneshot::Receiver<()>>,
}

impl PrewarmWait {
    /// True when there is nothing to wait for: every template the transaction needs is already
    /// compiled, or what was not could not be queued.
    pub fn is_empty(&self) -> bool {
        self.receivers.is_empty()
    }

    /// Complete once every compile this wait covers has finished, or [`PrewarmWait::timeout`]
    /// elapses.
    ///
    /// The timeout abandons the wait, never the compile: the worker runs on, so an executor that
    /// asks for the template next joins the compile already in flight through the provider's
    /// per-address semaphore. Consensus never waits on a bound of its own — a `get_template` that
    /// joins an in-flight compile is unbounded, because there is no execution result without the
    /// artifact and abandoning the join to compile the same module again is strictly worse.
    pub async fn wait(self) {
        let timeout = self.timeout();
        self.wait_for(timeout).await;
    }

    /// Allowance for the compiles this wait covers, which is one unit per template that was cold
    /// when the transaction was admitted. Three resident templates and one new one is one unit.
    fn timeout(&self) -> Duration {
        let cold_unique_templates = u32::try_from(self.receivers.len()).unwrap_or(u32::MAX);
        (PREWARM_WAIT_PER_TEMPLATE * cold_unique_templates).min(PREWARM_WAIT_CEILING)
    }

    async fn wait_for(self, timeout: Duration) {
        if self.receivers.is_empty() {
            return;
        }

        // A sender is dropped rather than sent on, so every outcome — compiled, failed, unknown
        // template — arrives here as a completed receiver.
        let all = futures::future::join_all(self.receivers);
        if tokio::time::timeout(timeout, all).await.is_err() {
            debug!(target: LOG_TARGET, "Prewarm did not finish within {timeout:?}");
        }
    }
}

/// The component an instruction calls a method on, whose template has to be looked up.
fn called_component(instruction: &Instruction) -> Option<ComponentAddress> {
    match instruction {
        Instruction::CallMethod {
            call: ComponentReference::Address(component),
            ..
        } => Some(*component),
        _ => None,
    }
}

/// The template an instruction will load, where that is knowable before execution. `instantiated`
/// answers for the components [`called_component`] named.
///
/// A `PublishTemplate` yields nothing: its address exists only once its substate is committed, so
/// the template it publishes is kept by the publish execution itself and, for a node that learns of
/// one without executing its publish, by [`TemplatePrewarmHooks`].
fn template_for(
    instruction: &Instruction,
    instantiated: &HashMap<ComponentAddress, TemplateAddress>,
) -> Option<TemplateAddress> {
    match instruction {
        Instruction::CallFunction { address, .. } => Some(*address),
        // `update_component_template` loads the template replacing the component's, and never the
        // one it replaces.
        Instruction::UpdateComponentTemplate { new_template, .. } => Some(*new_template),
        _ => instantiated.get(&called_component(instruction)?).copied(),
    }
}

/// Resolves the templates components instantiate.
///
/// Its own seam because a `CallMethod` names a component, not a template, and everything else the
/// pool does needs only the template provider.
pub trait ComponentTemplateLookup: Send + Sync + 'static {
    /// The template each of `components` instantiates, for those this node holds. A component in
    /// another shard group is absent from this node's state, and its template is not one this node
    /// executes against, so it is left out of the result.
    ///
    /// Takes the whole set at once because the caller is the mempool's own task: one transaction is
    /// one resolution, not one per instruction.
    fn templates_of(&self, components: &[ComponentAddress]) -> HashMap<ComponentAddress, TemplateAddress>;
}

/// Reads components out of the local state store at their latest version.
#[derive(Debug, Clone)]
pub struct StateStoreComponentLookup<TStore>(TStore);

impl<TStore: StateStore + Send + Sync + 'static> ComponentTemplateLookup for StateStoreComponentLookup<TStore> {
    fn templates_of(&self, components: &[ComponentAddress]) -> HashMap<ComponentAddress, TemplateAddress> {
        if components.is_empty() {
            return HashMap::new();
        }

        let ids: Vec<_> = components.iter().copied().map(SubstateId::Component).collect();
        let records = match self.0.with_read_tx(|tx| tx.substates_get_any_max_version(ids.iter())) {
            Ok(records) => records,
            Err(e) => {
                debug!(target: LOG_TARGET, "Prewarm could not read {} component(s): {e}", components.len());
                return HashMap::new();
            },
        };

        records
            .into_iter()
            .filter_map(|record| {
                let component = record.substate_id.as_component_address()?;
                let template = *record.into_substate_value()?.component()?.template_address();
                Some((component, template))
            })
            .collect()
    }
}

/// Start the prewarm pool and return the handle its triggers enqueue through.
///
/// `provider` must be a clone of the provider the executor uses, which is what lets a prewarm and an
/// execution of the same template coalesce onto one compile.
///
/// The workers are OS threads rather than tokio tasks: a compile is CPU-bound and synchronous, and
/// would hold a runtime worker for its whole duration. They run until the handle and all its clones
/// are dropped, which closes the queue.
pub fn spawn<TProvider, TStore>(
    provider: TProvider,
    store: TStore,
    #[cfg(feature = "metrics")] registry: &mut prometheus_client::registry::Registry,
) -> TemplatePrewarmer
where
    TProvider: TemplateProvider + ResidentTemplateProvider,
    TStore: StateStore + Send + Sync + 'static,
{
    let num_workers = worker_count();
    let (tx, rx) = mpsc::sync_channel(QUEUE_CAPACITY);
    let queued: Queued = Arc::new(Mutex::new(HashMap::new()));
    #[cfg(feature = "metrics")]
    let metrics = PrometheusPrewarmMetrics::new(registry);

    // One receiver shared by the pool. A worker holds the lock only while waiting for the next
    // address, so the thread that takes one releases the lock before compiling it and the next
    // worker starts waiting immediately.
    let rx = Arc::new(Mutex::new(rx));
    for i in 0..num_workers {
        let worker = Worker {
            rx: rx.clone(),
            queued: queued.clone(),
            provider: provider.clone(),
            #[cfg(feature = "metrics")]
            metrics: metrics.clone(),
        };
        thread::Builder::new()
            .name(format!("template-prewarm-{i}"))
            .spawn(move || worker.run())
            .expect("failed to spawn template prewarm worker");
    }

    info!(target: LOG_TARGET, "🔥 Template prewarm pool running with {num_workers} worker(s)");

    TemplatePrewarmer {
        tx,
        queued,
        residency: Arc::new(provider),
        components: Arc::new(StateStoreComponentLookup(store)),
        #[cfg(feature = "metrics")]
        metrics,
    }
}

/// Workers are sized off the machine rather than configured, and left well under both the core count
/// and [`MAX_WORKERS`]: prewarming competes with consensus execution for the same cores, and the
/// latency it removes is not worth the latency it would add by crowding out an executing block.
fn worker_count() -> usize {
    let cores = thread::available_parallelism().map_or(1, |n| n.get());
    (cores / 4).clamp(1, MAX_WORKERS)
}

struct Worker<TProvider> {
    rx: Arc<Mutex<Receiver<TemplateAddress>>>,
    queued: Queued,
    provider: TProvider,
    #[cfg(feature = "metrics")]
    metrics: PrometheusPrewarmMetrics,
}

impl<TProvider> Worker<TProvider>
where TProvider: TemplateProvider + ResidentTemplateProvider
{
    fn run(self) {
        loop {
            let address = {
                let rx = self.rx.lock().unwrap_or_else(PoisonError::into_inner);
                rx.recv()
            };
            let Ok(address) = address else {
                debug!(target: LOG_TARGET, "Prewarm queue closed, worker exiting");
                return;
            };

            self.prewarm(&address);

            // Held in the queued map until the work is done, so that a template wanted again while
            // this compile is in flight collects a waiter rather than a second queue slot and a
            // second worker. Removing it releases every waiter, whatever the outcome.
            let _queue_depth = {
                let mut queued = self.queued.lock().unwrap_or_else(PoisonError::into_inner);
                queued.remove(&address);
                queued.len()
            };
            #[cfg(feature = "metrics")]
            self.metrics.on_finished(_queue_depth);
        }
    }

    fn prewarm(&self, address: &TemplateAddress) {
        if self.provider.is_resident(address) {
            debug!(target: LOG_TARGET, "Template {address} was resident before its prewarm ran");
            #[cfg(feature = "metrics")]
            self.metrics.on_already_resident();
            return;
        }

        // Compiles and caches as a side effect. The template itself is of no interest here: the
        // point is that the executor's next lookup finds it resident.
        let started = Instant::now();
        match self.provider.get_template(address) {
            Ok(Some(_)) => {
                let elapsed = started.elapsed();
                debug!(target: LOG_TARGET, "Prewarmed template {address} in {elapsed:.1?}");
                #[cfg(feature = "metrics")]
                self.metrics.on_loaded(elapsed);
            },
            Ok(None) => {
                debug!(target: LOG_TARGET, "Template {address} is not known to this node");
                #[cfg(feature = "metrics")]
                self.metrics.on_not_found();
            },
            Err(e) => {
                debug!(target: LOG_TARGET, "Prewarm of template {address} failed: {e}");
                #[cfg(feature = "metrics")]
                self.metrics.on_failed();
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        sync::{
            atomic::{AtomicUsize, Ordering},
            mpsc::TryRecvError,
        },
    };

    use tari_crypto::{keys::SecretKey, ristretto::RistrettoSecretKey};
    use tari_ootle_common_types::Epoch;
    use tari_ootle_transaction::{TransactionBuilder, args};
    use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;

    use super::*;

    fn template(n: u8) -> TemplateAddress {
        TemplateAddress::from_array([n; 32])
    }

    #[derive(Clone, Default)]
    struct FakeProvider {
        loads: Arc<AtomicUsize>,
        resident: Arc<Mutex<HashSet<TemplateAddress>>>,
    }

    impl FakeProvider {
        fn with_resident(address: TemplateAddress) -> Self {
            let provider = Self::default();
            provider.resident.lock().unwrap().insert(address);
            provider
        }

        fn loads(&self) -> usize {
            self.loads.load(Ordering::SeqCst)
        }
    }

    #[derive(Debug, thiserror::Error)]
    #[error("no template")]
    struct NoTemplate;

    impl TemplateProvider for FakeProvider {
        type Error = NoTemplate;
        type Template = ();

        fn get_template(&self, address: &TemplateAddress) -> Result<Option<Self::Template>, Self::Error> {
            self.loads.fetch_add(1, Ordering::SeqCst);
            self.resident.lock().unwrap().insert(*address);
            Ok(Some(()))
        }
    }

    impl ResidentTemplateProvider for FakeProvider {
        fn is_resident(&self, address: &TemplateAddress) -> bool {
            self.resident.lock().unwrap().contains(address)
        }
    }

    #[derive(Default)]
    struct FakeComponents(HashMap<ComponentAddress, TemplateAddress>);

    impl FakeComponents {
        fn holding(component: ComponentAddress, template: TemplateAddress) -> Self {
            Self([(component, template)].into_iter().collect())
        }
    }

    impl ComponentTemplateLookup for FakeComponents {
        fn templates_of(&self, components: &[ComponentAddress]) -> HashMap<ComponentAddress, TemplateAddress> {
            components.iter().filter_map(|c| Some((*c, *self.0.get(c)?))).collect()
        }
    }

    /// A handle whose queue nothing drains, so that a test sees exactly what was enqueued.
    fn undrained(provider: FakeProvider) -> (TemplatePrewarmer, Receiver<TemplateAddress>) {
        undrained_with(provider, FakeComponents::default())
    }

    fn undrained_with(
        provider: FakeProvider,
        components: FakeComponents,
    ) -> (TemplatePrewarmer, Receiver<TemplateAddress>) {
        let (tx, rx) = mpsc::sync_channel(QUEUE_CAPACITY);
        let prewarmer = TemplatePrewarmer {
            tx,
            queued: Arc::new(Mutex::new(HashMap::new())),
            residency: Arc::new(provider),
            components: Arc::new(components),
            #[cfg(feature = "metrics")]
            metrics: PrometheusPrewarmMetrics::new(&mut prometheus_client::registry::Registry::default()),
        };
        (prewarmer, rx)
    }

    fn component(n: u8) -> ComponentAddress {
        ComponentAddress::from_array([n; 32])
    }

    /// Builds and seals a transaction over `build`'s instructions.
    fn transaction(build: impl FnOnce(TransactionBuilder) -> TransactionBuilder) -> Transaction {
        let secret = RistrettoSecretKey::random(&mut rand::rng());
        build(Transaction::builder_localnet(Epoch(1))).build_and_seal(&secret)
    }

    /// Every address a queue holds, in the order it was offered.
    fn queued_in(rx: &Receiver<TemplateAddress>) -> Vec<TemplateAddress> {
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    }

    /// Runs one worker over `addresses` and returns once it has drained them all.
    fn drain(provider: FakeProvider, addresses: &[TemplateAddress]) -> Queued {
        let (tx, rx) = mpsc::sync_channel(QUEUE_CAPACITY);
        let queued: Queued = Arc::new(Mutex::new(HashMap::new()));
        for address in addresses {
            queued.lock().unwrap().insert(*address, Vec::new());
            tx.send(*address).unwrap();
        }
        drop(tx);

        let worker = Worker {
            rx: Arc::new(Mutex::new(rx)),
            queued: queued.clone(),
            provider,
            #[cfg(feature = "metrics")]
            metrics: PrometheusPrewarmMetrics::new(&mut prometheus_client::registry::Registry::default()),
        };
        // The worker returns when the closed queue runs dry, which is what bounds this test.
        thread::spawn(move || worker.run()).join().unwrap();
        queued
    }

    #[test]
    fn a_builtin_is_never_queued() {
        let (prewarmer, rx) = undrained(FakeProvider::default());
        prewarmer.prewarm_template(ACCOUNT_TEMPLATE_ADDRESS);
        assert!(prewarmer.queued().is_empty());
        assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));
    }

    #[test]
    fn a_resident_template_is_never_queued() {
        let (prewarmer, rx) = undrained(FakeProvider::with_resident(template(1)));
        assert!(prewarmer.enqueue(template(1)).is_none());
        assert!(prewarmer.queued().is_empty());
        assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));
    }

    #[test]
    fn a_template_already_queued_collects_a_waiter_rather_than_a_slot() {
        let (prewarmer, rx) = undrained(FakeProvider::default());
        assert!(prewarmer.enqueue(template(1)).is_some());
        assert!(prewarmer.enqueue(template(1)).is_some());
        assert_eq!(prewarmer.queued()[&template(1)].len(), 2);
        assert_eq!(rx.try_recv(), Ok(template(1)));
        assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));
    }

    #[test]
    fn a_full_queue_drops_rather_than_blocks() {
        let (prewarmer, _rx) = undrained(FakeProvider::default());
        for i in 0..QUEUE_CAPACITY + 10 {
            let mut bytes = [0u8; 32];
            bytes[..8].copy_from_slice(&(i as u64).to_le_bytes());
            prewarmer.prewarm_template(TemplateAddress::from_array(bytes));
        }
        assert_eq!(prewarmer.queued().len(), QUEUE_CAPACITY);

        // A dropped request leaves the caller with nothing to wait for.
        let mut bytes = [0u8; 32];
        bytes[..8].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(prewarmer.enqueue(TemplateAddress::from_array(bytes)).is_none());
    }

    #[tokio::test]
    async fn a_wait_completes_when_its_compile_does() {
        let (prewarmer, rx) = undrained(FakeProvider::default());
        let wait = PrewarmWait {
            receivers: vec![prewarmer.enqueue(template(1)).unwrap()],
        };
        assert_eq!(rx.try_recv(), Ok(template(1)));

        // What a worker does when it finishes an address.
        prewarmer.queued().remove(&template(1));

        tokio::time::timeout(Duration::from_secs(5), wait.wait())
            .await
            .expect("wait outlived its own timeout");
    }

    #[tokio::test]
    async fn a_wait_gives_up_on_its_timeout_without_cancelling_the_compile() {
        let (prewarmer, _rx) = undrained(FakeProvider::default());
        let wait = PrewarmWait {
            receivers: vec![prewarmer.enqueue(template(1)).unwrap()],
        };

        wait.wait_for(Duration::from_millis(50)).await;

        assert!(
            prewarmer.queued().contains_key(&template(1)),
            "the timeout must leave the compile queued",
        );
    }

    #[tokio::test]
    async fn a_wait_on_nothing_completes_immediately() {
        let (prewarmer, _rx) = undrained(FakeProvider::with_resident(template(1)));
        let wait = PrewarmWait {
            receivers: vec![prewarmer.enqueue(template(1))].into_iter().flatten().collect(),
        };
        wait.wait().await;
    }

    #[test]
    fn the_wait_scales_with_the_cold_templates_and_stops_at_the_ceiling() {
        let (prewarmer, _rx) = undrained(FakeProvider::default());
        let wait_for = |n: u8| PrewarmWait {
            receivers: (1..=n).filter_map(|i| prewarmer.enqueue(template(i))).collect(),
        };

        assert_eq!(wait_for(0).timeout(), Duration::ZERO);
        assert_eq!(wait_for(1).timeout(), PREWARM_WAIT_PER_TEMPLATE);
        assert_eq!(wait_for(3).timeout(), PREWARM_WAIT_PER_TEMPLATE * 3);
        assert_eq!(wait_for(64).timeout(), PREWARM_WAIT_CEILING);
    }

    #[test]
    fn a_template_named_twice_is_waited_on_once() {
        let (prewarmer, _rx) = undrained(FakeProvider::default());
        assert!(prewarmer.enqueue(template(1)).is_some());
        assert!(prewarmer.enqueue(template(1)).is_some());
        assert_eq!(
            prewarmer.queued()[&template(1)].len(),
            2,
            "both callers wait on the one compile"
        );
    }

    #[test]
    fn a_call_function_queues_the_template_it_names() {
        let (prewarmer, rx) = undrained(FakeProvider::default());
        let tx = transaction(|b| b.call_function(template(1), "new", args![]));

        let wait = prewarmer.prewarm_transaction(&tx);

        assert!(!wait.is_empty());
        assert_eq!(queued_in(&rx), vec![template(1)]);
    }

    #[test]
    fn a_call_method_queues_the_template_its_component_instantiates() {
        let (prewarmer, rx) = undrained_with(
            FakeProvider::default(),
            FakeComponents::holding(component(7), template(2)),
        );
        let tx = transaction(|b| b.call_method(component(7), "withdraw", args![]));

        let wait = prewarmer.prewarm_transaction(&tx);

        assert!(!wait.is_empty());
        assert_eq!(queued_in(&rx), vec![template(2)]);
    }

    #[test]
    fn a_component_this_node_does_not_hold_is_no_work() {
        let (prewarmer, rx) = undrained(FakeProvider::default());
        let tx = transaction(|b| b.call_method(component(7), "withdraw", args![]));

        let wait = prewarmer.prewarm_transaction(&tx);

        assert!(wait.is_empty());
        assert!(queued_in(&rx).is_empty());
    }

    #[test]
    fn an_update_queues_the_template_that_will_replace_the_component_s() {
        let (prewarmer, rx) = undrained_with(
            FakeProvider::default(),
            FakeComponents::holding(component(7), template(2)),
        );
        let tx = transaction(|b| b.update_component_template(component(7), template(3)));

        let _wait = prewarmer.prewarm_transaction(&tx);

        assert_eq!(
            queued_in(&rx),
            vec![template(3)],
            "the template being replaced is never loaded, so compiling it is work thrown away",
        );
    }

    #[test]
    fn one_template_reached_two_ways_is_waited_on_once() {
        let (prewarmer, rx) = undrained_with(
            FakeProvider::default(),
            FakeComponents::holding(component(7), template(1)),
        );
        let tx = transaction(|b| {
            b.call_function(template(1), "new", args![])
                .call_method(component(7), "withdraw", args![])
        });

        let wait = prewarmer.prewarm_transaction(&tx);

        assert_eq!(queued_in(&rx), vec![template(1)]);
        assert_eq!(wait.receivers.len(), 1);
    }

    #[test]
    fn a_transaction_whose_templates_are_resident_waits_for_nothing() {
        let (prewarmer, rx) = undrained_with(
            FakeProvider::with_resident(template(1)),
            FakeComponents::holding(component(7), ACCOUNT_TEMPLATE_ADDRESS),
        );
        let tx = transaction(|b| {
            b.call_function(template(1), "new", args![])
                .call_method(component(7), "withdraw", args![])
        });

        assert!(prewarmer.prewarm_transaction(&tx).is_empty());
        assert!(queued_in(&rx).is_empty());
    }

    #[test]
    fn a_template_that_is_already_resident_is_not_loaded_again() {
        let provider = FakeProvider::with_resident(template(1));
        drain(provider.clone(), &[template(1)]);
        assert_eq!(provider.loads(), 0);
    }

    #[test]
    fn a_drained_template_leaves_the_queue() {
        let provider = FakeProvider::default();
        let queued = drain(provider.clone(), &[template(3)]);
        assert_eq!(provider.loads(), 1);
        assert!(queued.lock().unwrap().is_empty());
    }
}
