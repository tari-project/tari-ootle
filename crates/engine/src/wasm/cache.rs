//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! On-disk cache for compiled wasmer modules.
//!
//! Cranelift compilation of WASM templates is expensive: ~6 MB peak heap and
//! tens of milliseconds per template, paid on every node startup. The compiled
//! output for a given `(wasm_source, engine_config)` is deterministic and
//! reusable, so it can be persisted on local disk and loaded back via
//! [`wasmer::Module::deserialize_unchecked`] in milliseconds with negligible
//! peak heap.
//!
//! The WASM source bytes themselves stay on-chain (canonical, deterministic
//! representation). This cache is strictly node-local: any corrupt or missing
//! entry falls back to a full compile from source, with no consensus
//! implication.
//!
//! Two surfaces are exposed:
//!
//! - [`WasmModuleCache`] — low-level helper for callers that sit outside a `TemplateProvider` chain (e.g. the indexer's
//!   `TemplateManager`).
//! - [`DiskCachedWasmTemplateProvider`] — a `TemplateProvider` middleware that wraps a raw `PublishedTemplate` provider
//!   and outputs `LoadedTemplate`, doing compile-or-deserialize behind the scenes.

use std::{
    collections::HashMap,
    fs,
    io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        Mutex,
        MutexGuard,
        PoisonError,
        atomic::{self, AtomicU64},
        mpsc,
    },
    thread,
    time::{Duration, SystemTime},
};

use log::*;
use memmap2::Mmap;
use tari_engine_types::{limits::ModuleShape, published_template::PublishedTemplate};
use tari_ootle_common_types::{
    Epoch,
    services::template_provider::{TemplateMetadataProvider, TemplateProvider, TemplateProviderMetadata},
};
use tari_template_builtin::is_builtin_template_address;
use tari_template_lib::types::TemplateAddress;

use crate::{
    template::{LoadedTemplate, TemplateLoaderError},
    wasm::WasmModule,
};

const LOG_TARGET: &str = "tari::engine::wasm::cache";

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Engine-config fingerprint embedded in cache filenames.
///
/// Bump this string whenever any of the following change, otherwise nodes
/// loading from a stale cache will misbehave (deserialize failures at best,
/// undefined behaviour at worst):
///
/// - [`crate::wasm::WasmModule::create_engine`] config (compiler flags, features bitset, middleware list, tunables).
/// - [`tari_engine_types::limits::MAX_WASM_POINTS_PER_CALL`], which the metering middleware bakes into the artifact as
///   the initial remaining-points global. `WasmProcess::metering_allowance` reads it back with `get_remaining_points`,
///   so a node serving a stale artifact meters against the old cap and diverges from one that compiled fresh.
/// - The `wasmer` crate version (the serialized artifact format is internal to wasmer and not part of any stable wire
///   spec).
/// - The order or meaning of the header's fields at an unchanged `HEADER_BYTES`. A reader that agrees with the writer
///   on the header's length but not on its field order recomputes a matching CRC over values it then reads from the
///   wrong bytes. A change to the length needs no bump: it moves the artifact's start, so the body no longer begins
///   with wasmer's magic and `deserialize_unchecked` rejects the file. A header that grows is caught one step earlier,
///   at a CRC read from artifact bytes.
/// - How the header's values are derived — `validate_module_structure`'s segment tally. A cache hit serves these
///   verbatim rather than recomputing them, and `instantiation_points` prices a call off them, so a node reading a file
///   written under an older derivation charges a different fee for the same transaction than one that compiled fresh.
///   These are consensus values, not accounting hints.
///
/// On a bump, old cache files become orphans (different filename suffix)
/// and the next compile-from-source rewrites under the new key.
pub const ENGINE_FINGERPRINT: &str = "v5";

/// Five 8-byte LE fields at the head of each cache file: the original WASM source byte count
/// followed by the four counts of [`ModuleShape`]. `wasmer::Module::serialize` preserves none of
/// them, and all are needed after a cache hit — the first for accounting (e.g. moka weighing), the
/// rest to price instantiation.
const HEADER_FIELD_COUNT: usize = 5;
const HEADER_FIELD_BYTES: usize = HEADER_FIELD_COUNT * 8;

/// Zero padding between the fields and the CRC. This is the knob that satisfies the alignment
/// assert below when the field count changes; the CRC stays a `u64`.
const HEADER_PAD_BYTES: usize = 0;

/// Offset of the 8-byte LE CRC32 that covers every header byte before it.
///
/// The fields sit outside the wasmer artifact, so `deserialize_unchecked` has no view of damage
/// confined to them, while the four shape counts price `instantiation_points` into a committed fee
/// receipt. The CRC is the only check that reaches those bytes.
const CRC_OFFSET: usize = HEADER_FIELD_BYTES + HEADER_PAD_BYTES;

/// The header fields, the pad and the CRC.
///
/// The total is a multiple of 16: the artifact starts at `HEADER_BYTES` into a page-aligned mmap
/// and its rkyv metadata a further 32 bytes in, where `rkyv::access_unchecked` reads an archived
/// root that wasmer aligns to `MetadataHeader::ALIGN` (16); `MetadataHeader::parse` enforces the
/// weaker 8-byte bound on the artifact itself.
const HEADER_BYTES: usize = CRC_OFFSET + 8;

const _: () = assert!(
    HEADER_BYTES.is_multiple_of(16),
    "the artifact's rkyv metadata must start 16-byte aligned: widen HEADER_PAD_BYTES",
);

/// Infix marking a `store` tempfile, which no reader ever names.
const TMP_INFIX: &str = ".bin.tmp.";

/// How long a tempfile must have gone untouched before [`WasmModuleCache::open`] treats it as the
/// residue of a run that died between its write and its rename. A live `store` publishes in
/// milliseconds, so the margin only has to clear a stalled flush.
const STALE_TEMPFILE_AGE: Duration = Duration::from_secs(300);

/// What the cache knows about one file it has seen.
#[derive(Debug)]
struct IndexEntry {
    size_bytes: u64,
    /// Tick of the entry's last hit or store. Only the ordering carries meaning.
    last_used: u64,
    /// Identity of the file this entry describes, on platforms that expose one. An unlink checks it
    /// so that a file a concurrent `store` has since renamed into place is left alone.
    identity: Option<u64>,
}

/// Bookkeeping over the cache directory: what is in it, how much it weighs, and in what order it
/// was last wanted.
///
/// The directory remains the authority on contents. This is a running tally kept so that `store`
/// can answer "am I over the cap, and what is coldest?" without a `readdir` and a `stat` per file
/// on the execution path. A tally that drifts from the directory costs a recompile, never
/// correctness: an entry the index has lost is simply an unaccounted file, and one it invents is
/// dropped the next time its unlink finds nothing.
#[derive(Debug)]
struct CacheIndex {
    entries: HashMap<TemplateAddress, IndexEntry>,
    total_bytes: u64,
    cap_bytes: u64,
    clock: u64,
}

impl CacheIndex {
    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    fn record(&mut self, addr: TemplateAddress, size_bytes: u64, identity: Option<u64>) {
        let last_used = self.tick();
        if let Some(previous) = self.entries.insert(addr, IndexEntry {
            size_bytes,
            last_used,
            identity,
        }) {
            self.total_bytes = self.total_bytes.saturating_sub(previous.size_bytes);
        }
        self.total_bytes = self.total_bytes.saturating_add(size_bytes);
    }

    fn forget(&mut self, addr: &TemplateAddress) {
        if let Some(entry) = self.entries.remove(addr) {
            self.total_bytes = self.total_bytes.saturating_sub(entry.size_bytes);
        }
    }

    /// Removes and returns the least recently used entry.
    ///
    /// The last entry is never taken: a single artifact larger than the whole cap would otherwise be
    /// evicted by the very `store` that wrote it, and every call would recompile it.
    fn take_coldest(&mut self) -> Option<(TemplateAddress, IndexEntry)> {
        if self.entries.len() <= 1 {
            return None;
        }
        let addr = *self
            .entries
            .iter()
            .min_by_key(|(_, entry)| entry.last_used)
            .map(|(addr, _)| addr)?;
        let entry = self.entries.remove(&addr)?;
        self.total_bytes = self.total_bytes.saturating_sub(entry.size_bytes);
        Some((addr, entry))
    }
}

/// The template an artifact filename names, if it is this build's.
fn parse_artifact_name(name: &str, suffix: &str) -> Option<TemplateAddress> {
    TemplateAddress::from_hex(name.strip_suffix(suffix)?).ok()
}

/// The template an artifact filename names, if it belongs to a build with a different
/// [`ENGINE_FINGERPRINT`].
///
/// The shape is `{template address}_{fingerprint}.bin`. A name this does not parse belongs to
/// something other than this cache and is left where it is.
fn parse_orphan_name(name: &str) -> Option<TemplateAddress> {
    let stem = name.strip_suffix(".bin")?;
    let (addr, fingerprint) = stem.rsplit_once('_')?;
    if fingerprint == ENGINE_FINGERPRINT {
        return None;
    }
    TemplateAddress::from_hex(addr).ok()
}

/// The identity a later unlink compares against, where the platform has one.
#[cfg(unix)]
fn identity_of(meta: &fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(meta.ino())
}

#[cfg(not(unix))]
fn identity_of(_meta: &fs::Metadata) -> Option<u64> {
    None
}

/// Unlink `path`, unless it now names a different file than `identity` describes.
///
/// Both eviction and the corrupt-file paths delete by path, while `store` publishes by renaming a
/// new inode over that same path, so a delete decided against the file one call read can land on the
/// file another call has just published. The check narrows that window to the gap between the stat
/// and the unlink, on platforms that expose an identity; elsewhere the unlink is unconditional. What
/// is left costs a recompile.
fn remove_tracked_file(path: &Path, identity: Option<u64>) {
    if let Some(identity) = identity {
        match fs::metadata(path) {
            Ok(meta) if identity_of(&meta) != Some(identity) => {
                debug!(
                    target: LOG_TARGET,
                    "Leaving {} alone: it was replaced since it was read", path.display(),
                );
                return;
            },
            Ok(_) => {},
            Err(_) => return,
        }
    }
    let _ignore = fs::remove_file(path);
}

/// Low-level on-disk cache for compiled wasmer modules.
///
/// Files live at `{dir}/{template_address}_{ENGINE_FINGERPRINT}.bin`.
/// The body is `[u64 LE: code_size][u64 LE x4: ModuleShape][u64 LE: CRC32 of the preceding fields]
/// || wasmer::Module::serialize(...)`.
///
/// Writes are atomic (tempfile + rename). Read failures (missing file,
/// deserialize errors, format changes) are non-fatal: the corrupt file is
/// removed and the caller is expected to recompile from source.
///
/// The cache is bounded by total bytes rather than by entry count: a compiled artifact runs about
/// ten times the size of the WASM it came from, and the range across real templates is wide enough
/// (hundreds of KiB to tens of MiB) that a count says little about disk. Over the cap, the least
/// recently used artifacts are unlinked.
///
/// Eviction is node-local policy with no consensus reach, because an evicted artifact costs only a
/// recompile of bytes that remain on-chain.
///
/// [`WasmModuleCache::store`] hands its artifact to a writer thread instead of writing it. Writing
/// one means serializing a multi-megabyte artifact and flushing it to the device, and `store`'s
/// caller is an execution: on a validator that is consensus, on the indexer a dry run. What the
/// caller needs from a store is that the artifact is on disk before it is wanted again, which is
/// the next transaction to call the template at the earliest, not before the call that compiled it
/// returns.
#[derive(Debug, Clone)]
pub struct WasmModuleCache {
    dir: PathBuf,
    index: Arc<Mutex<CacheIndex>>,
    writes: mpsc::SyncSender<WriteRequest>,
}

/// Artifacts the writer thread will accept before [`WasmModuleCache::store`] starts dropping them.
///
/// Queueing one is a cheap clone — a `wasmer::Module` is an `Arc` — but the queue outlives the
/// execution that compiled it, so a full queue is the last owner of this many artifacts, which this
/// module's own docs put at hundreds of KiB to tens of MiB each. The queue's job is to decouple a
/// caller from one slow write rather than to hold a cache's worth of compiled code, so it is sized
/// for the first. A dropped artifact costs a recompile the next time its template is wanted cold.
const WRITE_QUEUE_CAPACITY: usize = 8;

/// A request to the writer thread.
enum WriteRequest {
    Artifact {
        addr: TemplateAddress,
        loaded: LoadedTemplate,
    },
    /// Answered once every artifact queued before it has been written.
    #[cfg(test)]
    Barrier(mpsc::SyncSender<()>),
}

impl WasmModuleCache {
    /// Open or create a cache rooted at `dir`, holding at most `cap_bytes` of artifacts. Creates the
    /// directory tree if missing.
    ///
    /// A directory takes one instance per process, cloned to each of its consumers rather than opened again by
    /// each, so that what the handle tracks covers the whole directory.
    ///
    /// Opening walks the directory to tally what is already there, and takes the chance to clear
    /// tempfiles no live `store` is writing to. Anything over the cap — a cache from a run
    /// configured with a larger one — is evicted before the first lookup.
    pub fn open(dir: impl Into<PathBuf>, cap_bytes: u64) -> io::Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        let (writes, requests) = mpsc::sync_channel(WRITE_QUEUE_CAPACITY);
        let cache = Self {
            index: Arc::new(Mutex::new(Self::build_index(&dir, cap_bytes)?)),
            dir,
            writes,
        };
        cache.evict_to_fit();
        cache.spawn_writer(requests);
        // A default that decides how much disk this node uses and what it deletes is logged
        // unconditionally: an operator who upgraded without touching their config should be able to
        // see what changed underneath them in their own logs.
        info!(
            target: LOG_TARGET,
            "⚙️ Wasm module cache at {} holds {} artifact(s), {} bytes of a {} byte cap",
            cache.dir.display(),
            cache.len(),
            cache.total_bytes(),
            cap_bytes,
        );
        Ok(cache)
    }

    /// Tallies the cache directory, and clears what no build can use: tempfiles no live `store` is
    /// writing, and artifacts under a fingerprint other than this build's.
    ///
    /// Reaping other fingerprints is what makes the cap a bound on the directory rather than on one
    /// generation of it. A fingerprint bump follows any wasmer upgrade or metering change, and the
    /// build that wrote the previous generation is the one being replaced, so nothing else will ever
    /// collect those files. The cost of getting it wrong — a directory shared with an older binary,
    /// which then finds its artifacts gone — is a recompile per template on that binary's next run.
    ///
    /// Recency is seeded from modification time, the only ordering the filesystem retains across a
    /// restart. Hits are not written back to it: a `utimes` per cache hit buys a better order after
    /// the next restart at the cost of a write on every read, and a mis-ordered eviction costs one
    /// recompile.
    fn build_index(dir: &Path, cap_bytes: u64) -> io::Result<CacheIndex> {
        let suffix = format!("_{ENGINE_FINGERPRINT}.bin");
        let stale_before = SystemTime::now().checked_sub(STALE_TEMPFILE_AGE);
        let mut found = Vec::new();

        for entry in fs::read_dir(dir)? {
            let Ok(entry) = entry else { continue };
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }

            // A tempfile is a partial write, valid for no build, so the age gate is the only
            // question: a live `store` may still be writing it.
            if name.contains(TMP_INFIX) {
                let is_abandoned =
                    stale_before.is_some_and(|stale_before| meta.modified().is_ok_and(|m| m < stale_before));
                if is_abandoned {
                    debug!(target: LOG_TARGET, "Reaping abandoned cache tempfile {}", name);
                    let _ignore = fs::remove_file(entry.path());
                }
                continue;
            }

            let Some(addr) = parse_artifact_name(name, &suffix) else {
                if let Some(addr) = parse_orphan_name(name) {
                    debug!(
                        target: LOG_TARGET,
                        "Reaping cached module for template {} under a superseded fingerprint", addr,
                    );
                    let _ignore = fs::remove_file(entry.path());
                }
                continue;
            };
            found.push((
                addr,
                meta.len(),
                meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                identity_of(&meta),
            ));
        }

        found.sort_by_key(|(_, _, modified, _)| *modified);

        let mut index = CacheIndex {
            entries: HashMap::with_capacity(found.len()),
            total_bytes: 0,
            cap_bytes,
            clock: 0,
        };
        for (addr, size_bytes, _, identity) in found {
            index.record(addr, size_bytes, identity);
        }
        Ok(index)
    }

    /// A poisoned index is recovered rather than propagated: it is a tally over a directory that
    /// remains the authority, so the worst a panic mid-update leaves behind is a wrong byte total.
    fn index(&self) -> MutexGuard<'_, CacheIndex> {
        self.index.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Unlinks least-recently-used artifacts until the cache is within its cap.
    fn evict_to_fit(&self) {
        // The unlinks run outside the lock: only the tally has to be consistent, and a victim is
        // already out of it by the time its file goes.
        let mut victims = Vec::new();
        {
            let mut index = self.index();
            while index.total_bytes > index.cap_bytes {
                let Some(victim) = index.take_coldest() else {
                    break;
                };
                victims.push(victim);
            }
        }
        for (addr, entry) in victims {
            debug!(
                target: LOG_TARGET,
                "Evicting cached module for template {} ({} bytes)", addr, entry.size_bytes,
            );
            remove_tracked_file(&self.path_for(&addr), entry.identity);
        }
    }

    /// Drops `addr` from the index and unlinks its file, if that file is still the one `identity`
    /// describes.
    fn discard(&self, addr: &TemplateAddress, path: &Path, identity: Option<u64>) {
        self.index().forget(addr);
        remove_tracked_file(path, identity);
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path_for(&self, addr: &TemplateAddress) -> PathBuf {
        self.dir.join(format!("{}_{}.bin", addr, ENGINE_FINGERPRINT))
    }

    /// Try to load a previously-cached module for `addr`. Returns `None` on
    /// any miss — file missing, header malformed, deserialize failure. On
    /// recoverable corruption the bad file is removed so a subsequent `store`
    /// can replace it.
    ///
    /// The file is `mmap`'d rather than read into a `Vec<u8>` — wasmer's
    /// deserialize path accepts `bytes::Bytes` and `Bytes::from_owner` lets us
    /// hand it the mmap region without copying. Cache hits cost a single
    /// `mmap` syscall (and the page faults wasmer's deserializer triggers as
    /// it walks the artifact); no full-artifact allocation.
    pub fn try_load(&self, addr: &TemplateAddress) -> Option<LoadedTemplate> {
        let path = self.path_for(addr);
        let file = match fs::File::open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return None,
            Err(e) => {
                warn!(
                    target: LOG_TARGET,
                    "Failed to open cache file {}: {}", path.display(), e,
                );
                return None;
            },
        };
        let identity = file.metadata().ok().as_ref().and_then(identity_of);

        // SAFETY: see the docs on `Mmap::map`. A mapping SIGBUSes if the file shrinks under it, so
        // what keeps this sound is that `store` never writes through the published path: it fills a
        // tempfile only that call can name and renames, pointing the directory entry at a new inode
        // and leaving this mapping's inode whole. Writers under a different engine config target a
        // different filename through the fingerprint suffix.
        let mmap = match unsafe { Mmap::map(&file) } {
            Ok(m) => m,
            Err(e) => {
                warn!(
                    target: LOG_TARGET,
                    "Failed to mmap cache file {}: {}", path.display(), e,
                );
                // An empty file cannot be mapped at all, so it never reaches the length check below.
                // Other map failures are resource limits, under which the file may still be good.
                if file.metadata().is_ok_and(|m| m.len() == 0) {
                    self.discard(addr, &path, identity);
                }
                return None;
            },
        };

        if mmap.len() < HEADER_BYTES {
            warn!(
                target: LOG_TARGET,
                "Cache file {} is shorter than the {}-byte header; removing.",
                path.display(),
                HEADER_BYTES,
            );
            drop(mmap);
            self.discard(addr, &path, identity);
            return None;
        }

        let stored_crc = u64::from_le_bytes(
            mmap[CRC_OFFSET..HEADER_BYTES]
                .try_into()
                .expect("HEADER_BYTES - CRC_OFFSET is 8"),
        );
        let computed_crc = u64::from(crc32fast::hash(&mmap[..CRC_OFFSET]));
        if stored_crc != computed_crc {
            warn!(
                target: LOG_TARGET,
                "Cache file {} has a corrupt header (CRC {:#010x}, expected {:#010x}); removing.",
                path.display(),
                stored_crc,
                computed_crc,
            );
            drop(mmap);
            self.discard(addr, &path, identity);
            return None;
        }

        let mut field = [0u8; 8];
        let mut read_field = |i: usize| {
            field.copy_from_slice(&mmap[i * 8..(i + 1) * 8]);
            u64::from_le_bytes(field)
        };
        let code_size = read_field(0) as usize;
        let shape = ModuleShape {
            data_segment_bytes: read_field(1),
            data_segment_count: read_field(2),
            element_segment_entries: read_field(3),
            declared_table_slots: read_field(4),
        };

        // Wrap the mmap as a Bytes that owns it, then slice past the
        // header. `Bytes::slice` is zero-copy (pointer + length
        // adjustment); the wrapped Mmap is dropped only when the resulting
        // Bytes (and any clones the deserializer may keep) goes out of
        // scope.
        let size_bytes = mmap.len() as u64;
        let body = bytes::Bytes::from_owner(mmap).slice(HEADER_BYTES..);

        // SAFETY: bytes were written by [`Self::store`] in a previous run of
        // this process (or an earlier process owning the same data dir) via
        // `wasmer::Module::serialize`. The cache directory is node-local and
        // not attacker-controlled in any sane operational setup. The
        // fingerprint suffix in the filename guarantees the engine config
        // matches this build; a deserialize failure simply triggers the
        // recompile fallback.
        match unsafe { WasmModule::load_template_from_serialized(body, code_size, shape) } {
            Ok(loaded) => {
                debug!(target: LOG_TARGET, "Cache hit for template {}", addr);
                // A hit also adopts a file this instance never wrote, so a cache inherited from an
                // earlier run counts towards the cap from the first time it is wanted.
                self.index().record(*addr, size_bytes, identity);
                Some(loaded)
            },
            Err(err) => {
                warn!(
                    target: LOG_TARGET,
                    "Failed to deserialize cached module {}: {}; removing.",
                    path.display(),
                    err,
                );
                self.discard(addr, &path, identity);
                None
            },
        }
    }

    /// One thread per cache, so that artifacts are written in the order they were offered and two
    /// stores of one address never race for its filename.
    ///
    /// It ends when the cache and every clone of it are dropped, which closes the queue. An
    /// artifact still queued at that point is lost, and costs the recompile any artifact that was
    /// never written costs.
    fn spawn_writer(&self, requests: mpsc::Receiver<WriteRequest>) {
        let cache = self.clone_without_writer();
        let spawned = thread::Builder::new()
            .name("wasm-cache-writer".to_string())
            .spawn(move || {
                for request in requests {
                    match request {
                        WriteRequest::Artifact { addr, loaded } => cache.write_now(&addr, &loaded),
                        #[cfg(test)]
                        WriteRequest::Barrier(reply) => {
                            let _ignore = reply.send(());
                        },
                    }
                }
                debug!(target: LOG_TARGET, "Wasm module cache writer exiting");
            });
        if let Err(e) = spawned {
            warn!(
                target: LOG_TARGET,
                "Failed to spawn the wasm module cache writer: {}; artifacts will not be cached", e,
            );
        }
    }

    /// A handle onto the same directory and index whose own `writes` sender is disconnected.
    ///
    /// The writer thread holds this rather than a full clone: a clone would keep the queue's sender
    /// alive from inside the thread that drains it, so the queue would never close and the thread
    /// would never end.
    fn clone_without_writer(&self) -> Self {
        let (writes, _dropped) = mpsc::sync_channel(0);
        Self {
            dir: self.dir.clone(),
            index: self.index.clone(),
            writes,
        }
    }

    /// Offer a compiled module to the cache under `addr`.
    ///
    /// Returns as soon as the artifact is queued for the writer thread. Best-effort throughout: a
    /// full queue drops the artifact and any failure to write it is logged, because the caller's
    /// compiled module is valid either way and the only cost of no cache entry is a later
    /// recompile.
    pub fn store(&self, addr: &TemplateAddress, loaded: &LoadedTemplate) {
        let request = WriteRequest::Artifact {
            addr: *addr,
            loaded: loaded.clone(),
        };
        match self.writes.try_send(request) {
            Ok(_) => {},
            Err(mpsc::TrySendError::Full(_)) => {
                // Warned rather than logged quietly: a device slow enough to keep the writer behind
                // turns this cache into a directory nothing lands in, and every call to a cold
                // template recompiles it. Before the write moved off the caller that showed up as
                // slow execution, which an operator could see.
                warn!(
                    target: LOG_TARGET,
                    "The wasm module cache writer is behind. Dropping the compiled module for template {}, which \
                     will be compiled again when that template is next wanted cold.",
                    addr,
                );
            },
            Err(mpsc::TrySendError::Disconnected(_)) => {
                warn!(
                    target: LOG_TARGET,
                    "The wasm module cache writer has stopped. Nothing further will be cached for the life of this \
                     process, starting with the compiled module for template {}.",
                    addr,
                );
            },
        }
    }

    /// Serialize `loaded` and publish it at `addr`'s cache filename.
    fn write_now(&self, addr: &TemplateAddress, loaded: &LoadedTemplate) {
        let LoadedTemplate::Wasm(wasm) = loaded;
        let serialized = match wasm.wasm_module().serialize() {
            Ok(s) => s,
            Err(e) => {
                warn!(target: LOG_TARGET, "Failed to serialize module for {}: {}", addr, e);
                return;
            },
        };

        let path = self.path_for(addr);
        // Every call needs its own tempfile: another process pointing at this directory can be
        // publishing the same address at the same moment. The pid separates this process's writes
        // from theirs and the counter separates this call from the next.
        let tmp = self.dir.join(format!(
            "{}_{}.bin.tmp.{}.{}",
            addr,
            ENGINE_FINGERPRINT,
            std::process::id(),
            TMP_COUNTER.fetch_add(1, atomic::Ordering::Relaxed),
        ));

        let shape = wasm.shape();
        // `HEADER_FIELD_COUNT` fields, in the order `try_load` reads them.
        let fields: [u64; HEADER_FIELD_COUNT] = [
            wasm.code_size() as u64,
            shape.data_segment_bytes,
            shape.data_segment_count,
            shape.element_segment_entries,
            shape.declared_table_slots,
        ];
        let mut header = [0u8; HEADER_BYTES];
        for (slot, value) in header.chunks_exact_mut(8).zip(fields) {
            slot.copy_from_slice(&value.to_le_bytes());
        }
        let crc = u64::from(crc32fast::hash(&header[..CRC_OFFSET]));
        header[CRC_OFFSET..].copy_from_slice(&crc.to_le_bytes());

        let mut bytes = Vec::with_capacity(HEADER_BYTES + serialized.len());
        bytes.extend_from_slice(&header);
        bytes.extend_from_slice(&serialized);

        let identity = match write_durable(&tmp, &bytes) {
            Ok(identity) => identity,
            Err(e) => {
                warn!(target: LOG_TARGET, "Failed to write cache tempfile {}: {}", tmp.display(), e);
                let _ignore = fs::remove_file(&tmp);
                return;
            },
        };

        if let Err(e) = fs::rename(&tmp, &path) {
            warn!(
                target: LOG_TARGET,
                "Failed to rename {} -> {}: {}", tmp.display(), path.display(), e,
            );
            let _ignore = fs::remove_file(&tmp);
            return;
        }

        debug!(
            target: LOG_TARGET,
            "Cached compiled module for template {} -> {}", addr, path.display(),
        );

        self.index().record(*addr, bytes.len() as u64, identity);
        self.evict_to_fit();
    }

    /// Block until every artifact offered before this call has been written.
    #[cfg(test)]
    fn flush(&self) {
        let (reply, done) = mpsc::sync_channel(0);
        if self.writes.send(WriteRequest::Barrier(reply)).is_ok() {
            let _ignore = done.recv();
        }
    }

    /// Bytes of artifact this cache is tracking.
    pub fn total_bytes(&self) -> u64 {
        self.index().total_bytes
    }

    /// Artifacts this cache is tracking.
    pub fn len(&self) -> usize {
        self.index().entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Write `bytes` to `path`, flushed to the device before returning, and report the identity of the
/// file written.
///
/// [`WasmModuleCache::store`] publishes a file by rename, which can expose contents still held in
/// the page cache. The artifact body lies past the header CRC's coverage, so it must reach the
/// device before the rename names it. Durability stops at the contents: a rename lost to a crash
/// costs one recompile.
fn write_durable(path: &Path, bytes: &[u8]) -> io::Result<Option<u64>> {
    use std::io::Write;

    let mut file = fs::File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    // Taken before the rename: the identity travels with the inode, and reading it here cannot race
    // another call publishing over the destination.
    Ok(file.metadata().ok().as_ref().and_then(identity_of))
}

/// `TemplateProvider` middleware that adds an on-disk compiled-module cache
/// behind any provider returning raw [`PublishedTemplate`] bytes.
///
/// On `get_template(addr)`:
/// 1. If the cache file `{addr}_{ENGINE_FINGERPRINT}.bin` exists, deserialize and return — no compile, no
///    inner-provider call.
/// 2. Otherwise delegate to `inner` for the raw `PublishedTemplate`, compile via
///    [`WasmModule::load_template_from_code`], offer the compiled module to the cache, return. The artifact is written
///    by the cache's writer thread, so the caller pays the compile but not the flush.
///
/// Intended placement is between an outer in-memory cache (e.g. moka) and the
/// raw state-store provider, so a process-lifetime hot path skips disk
/// entirely and only the first compile-then-deserialize crossing per template
/// per node ever pays the disk cost.
#[derive(Debug, Clone)]
pub struct DiskCachedWasmTemplateProvider<TStore> {
    inner: TStore,
    cache: WasmModuleCache,
}

impl<TStore> DiskCachedWasmTemplateProvider<TStore> {
    pub fn new(inner: TStore, cache: WasmModuleCache) -> Self {
        Self { inner, cache }
    }

    /// Opens a cache at `path`, bounded to `cap_bytes`, for this provider's exclusive use. A process whose cache
    /// directory has another consumer opens it once with [`WasmModuleCache::open`] and passes a clone to
    /// [`Self::new`].
    pub fn open(inner: TStore, path: impl Into<PathBuf>, cap_bytes: u64) -> io::Result<Self> {
        let wasm_cache = WasmModuleCache::open(path, cap_bytes)?;
        Ok(Self::new(inner, wasm_cache))
    }
}

/// Both lookups below answer from a cache hit without asking `inner`. What makes that sound is a
/// property of the directory rather than of this type: every artifact in it is one whose template
/// substate had been resolved before it was written, and templates are immutable and are never
/// destroyed, so an artifact that was right when written stays right.
///
/// Each writer establishes that for itself, and there are two. This provider stores only what
/// `inner.get_template` has just served. The indexer's `TemplateManager` shares the same
/// `WasmModuleCache` and stores from its own `templates` table, whose rows reach `Active` only
/// through `add_and_load_template` after the template's substate was fetched.
///
/// A third writer owes this path the same guarantee. `call_function` resolves a template through the
/// provider and nothing else, so a node serving an artifact for a substate that does not exist
/// executes a call every other node aborts.
impl<TStore> TemplateProvider for DiskCachedWasmTemplateProvider<TStore>
where TStore: TemplateProvider<Template = PublishedTemplate> + Clone + 'static
{
    type Error = DiskCachedWasmTemplateProviderError;
    type Template = LoadedTemplate;

    fn get_template(&self, address: &TemplateAddress) -> Result<Option<Self::Template>, Self::Error> {
        // Builtins bypass the disk cache: their addresses are hardcoded
        // constants (independent of binary content), so a cache entry under
        // a builtin's address would silently serve an out-of-date compiled
        // module after a builtin recompile. User-template addresses are
        // content-addressed, so binary changes implicitly invalidate the
        // cache key.
        if is_builtin_template_address(address) {
            let Some(published) = self
                .inner
                .get_template(address)
                .map_err(|e| DiskCachedWasmTemplateProviderError::Inner(e.into()))?
            else {
                return Ok(None);
            };
            return Ok(Some(WasmModule::load_template_from_code(published.binary.as_slice())?));
        }

        if let Some(loaded) = self.cache.try_load(address) {
            return Ok(Some(loaded));
        }

        let Some(published) = self
            .inner
            .get_template(address)
            .map_err(|e| DiskCachedWasmTemplateProviderError::Inner(e.into()))?
        else {
            return Ok(None);
        };

        let loaded = WasmModule::load_template_from_code(published.binary.as_slice())?;
        self.cache.store(address, &loaded);
        Ok(Some(loaded))
    }

    fn has_template(&self, address: &TemplateAddress) -> Result<bool, Self::Error> {
        // Cheap path: cache hit implies the template exists. A miss falls
        // through to the inner provider, which is allowed to answer without
        // materialising the binary.
        if !is_builtin_template_address(address) && self.cache.path_for(address).exists() {
            return Ok(true);
        }
        self.inner
            .has_template(address)
            .map_err(|e| DiskCachedWasmTemplateProviderError::Inner(e.into()))
    }
}

impl<TStore> TemplateMetadataProvider for DiskCachedWasmTemplateProvider<TStore>
where TStore: TemplateProvider<Template = PublishedTemplate> + Clone + 'static
{
    fn get_template_metadata(&self, id: &TemplateAddress) -> Result<Option<TemplateProviderMetadata>, Self::Error> {
        // Metadata always reads from the underlying state store, never from
        // the disk cache (the cache only stores the compiled module, not the
        // PublishedTemplate's author / epoch / metadata_hash fields).
        let template = self
            .inner
            .get_template(id)
            .map_err(|e| DiskCachedWasmTemplateProviderError::Inner(e.into()))?;
        Ok(template.map(|t| TemplateProviderMetadata {
            author: t.author,
            binary_hash: t.to_binary_hash(),
            epoch: Epoch(t.at_epoch),
            metadata_hash: t.metadata_hash,
        }))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DiskCachedWasmTemplateProviderError {
    #[error("Inner template provider error: {0}")]
    Inner(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error(transparent)]
    TemplateLoader(#[from] TemplateLoaderError),
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tari_engine_types::published_template::PublishedTemplate;
    use tari_template_builtin::all_builtin_templates;
    use tari_template_lib::types::crypto::RistrettoPublicKeyBytes;
    use tempfile::TempDir;

    use super::*;

    #[derive(Clone)]
    struct StaticStore {
        templates: Arc<std::collections::HashMap<TemplateAddress, PublishedTemplate>>,
    }

    #[derive(Debug, thiserror::Error)]
    #[error("not found")]
    struct StaticStoreError;

    impl TemplateProvider for StaticStore {
        type Error = StaticStoreError;
        type Template = PublishedTemplate;

        fn get_template(&self, address: &TemplateAddress) -> Result<Option<Self::Template>, Self::Error> {
            Ok(self.templates.get(address).cloned())
        }
    }

    /// Comfortably larger than the one artifact the shared fixtures compile, so that a test that is
    /// not about eviction never trips it.
    const TEST_CAP_BYTES: u64 = 64 * 1024 * 1024;

    fn account_binary() -> &'static [u8] {
        all_builtin_templates()
            .iter()
            .find(|t| t.name == "Account")
            .expect("Account builtin")
            .binary
    }

    fn addr_of_byte(b: u8) -> TemplateAddress {
        let addr = TemplateAddress::from_array([b; 32]);
        debug_assert!(
            !is_builtin_template_address(&addr),
            "test address must not collide with a builtin",
        );
        addr
    }

    /// One compiled artifact, and the byte count a `store` of it writes.
    fn compiled_artifact() -> (LoadedTemplate, u64) {
        let dir = TempDir::new().unwrap();
        let cache = WasmModuleCache::open(dir.path(), TEST_CAP_BYTES).unwrap();
        let addr = addr_of_byte(0x01);
        let loaded = WasmModule::load_template_from_code(account_binary()).unwrap();
        cache.store(&addr, &loaded);
        cache.flush();
        let size = fs::metadata(cache.path_for(&addr)).unwrap().len();
        (loaded, size)
    }

    fn make_store() -> (StaticStore, TemplateAddress) {
        // We re-use the Account builtin's *binary* (it's a real, valid WASM
        // template available in dev-deps) but file it under a synthetic
        // non-builtin address — the disk-cache path bypasses real builtin
        // addresses by design (see is_builtin_template_address).
        let template = all_builtin_templates()
            .iter()
            .find(|t| t.name == "Account")
            .expect("Account builtin");
        let test_addr = TemplateAddress::from_array([
            0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42,
            0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42,
        ]);
        debug_assert!(
            !is_builtin_template_address(&test_addr),
            "test address must not collide with a builtin",
        );
        let mut map = std::collections::HashMap::new();
        let published = PublishedTemplate {
            template_name: template.name.try_into().expect("valid name"),
            author: RistrettoPublicKeyBytes::default(),
            binary: template.binary.to_vec().try_into().expect("template binary too large"),
            at_epoch: 0,
            metadata_hash: None,
        };
        map.insert(test_addr, published);
        (
            StaticStore {
                templates: Arc::new(map),
            },
            test_addr,
        )
    }

    #[test]
    fn round_trip_compile_then_deserialize() {
        let dir = TempDir::new().unwrap();
        let cache = WasmModuleCache::open(dir.path(), TEST_CAP_BYTES).unwrap();
        let (store, addr) = make_store();
        let provider = DiskCachedWasmTemplateProvider::new(store.clone(), cache.clone());

        // First call: cache miss, compile-then-store.
        let first = provider.get_template(&addr).unwrap().expect("loaded");
        cache.flush();
        assert!(cache.path_for(&addr).exists(), "store should write a file");

        // Second call: cache hit, deserialize-only path.
        let second = provider.get_template(&addr).unwrap().expect("loaded");
        assert_eq!(first.template_name(), second.template_name());
        assert_eq!(first.code_size(), second.code_size());
        assert_eq!(
            first.template_def().functions().len(),
            second.template_def().functions().len(),
        );
    }

    #[test]
    fn corrupt_cache_falls_back_to_recompile() {
        let dir = TempDir::new().unwrap();
        let cache = WasmModuleCache::open(dir.path(), TEST_CAP_BYTES).unwrap();
        let (store, addr) = make_store();

        // Plant garbage at the expected filename.
        let path = cache.path_for(&addr);
        fs::write(&path, b"this is not a wasmer artifact").unwrap();
        assert!(path.exists());

        // try_load should return None, having removed the corrupt file.
        assert!(cache.try_load(&addr).is_none());
        assert!(!path.exists(), "corrupt file should be removed");

        // Provider compiles fresh and writes a valid file.
        let provider = DiskCachedWasmTemplateProvider::new(store, cache.clone());
        provider.get_template(&addr).unwrap().expect("loaded");
        cache.flush();
        assert!(path.exists(), "fresh compile should re-populate the cache");

        // And the freshly-cached file deserializes cleanly.
        assert!(cache.try_load(&addr).is_some());
    }

    #[test]
    fn flipped_header_byte_falls_back_to_recompile() {
        let dir = TempDir::new().unwrap();
        let cache = WasmModuleCache::open(dir.path(), TEST_CAP_BYTES).unwrap();
        let (store, addr) = make_store();
        let provider = DiskCachedWasmTemplateProvider::new(store, cache.clone());
        provider.get_template(&addr).unwrap().expect("loaded");
        cache.flush();

        // Damage a shape count, leaving the wasmer artifact itself intact.
        let path = cache.path_for(&addr);
        let mut bytes = fs::read(&path).unwrap();
        bytes[8] ^= 0x01;
        fs::write(&path, &bytes).unwrap();

        assert!(cache.try_load(&addr).is_none(), "a bad header CRC is a miss");
        assert!(!path.exists(), "the file with the bad CRC should be removed");
    }

    #[test]
    fn flipped_crc_byte_falls_back_to_recompile() {
        let dir = TempDir::new().unwrap();
        let cache = WasmModuleCache::open(dir.path(), TEST_CAP_BYTES).unwrap();
        let (store, addr) = make_store();
        let provider = DiskCachedWasmTemplateProvider::new(store, cache.clone());
        provider.get_template(&addr).unwrap().expect("loaded");
        cache.flush();

        let path = cache.path_for(&addr);
        let mut bytes = fs::read(&path).unwrap();
        bytes[CRC_OFFSET] ^= 0x01;
        fs::write(&path, &bytes).unwrap();

        assert!(cache.try_load(&addr).is_none());
        assert!(!path.exists());
    }

    #[test]
    fn truncated_header_treated_as_miss() {
        let dir = TempDir::new().unwrap();
        let cache = WasmModuleCache::open(dir.path(), TEST_CAP_BYTES).unwrap();
        let (_store, addr) = make_store();

        let path = cache.path_for(&addr);
        fs::write(&path, vec![0u8; HEADER_BYTES - 1]).unwrap();

        assert!(cache.try_load(&addr).is_none());
        assert!(!path.exists(), "a short file should be removed");
    }

    #[test]
    fn stored_header_crc_covers_the_fields() {
        let dir = TempDir::new().unwrap();
        let cache = WasmModuleCache::open(dir.path(), TEST_CAP_BYTES).unwrap();
        let (store, addr) = make_store();
        let provider = DiskCachedWasmTemplateProvider::new(store, cache.clone());
        let loaded = provider.get_template(&addr).unwrap().expect("loaded");
        cache.flush();

        let bytes = fs::read(cache.path_for(&addr)).unwrap();
        let stored_crc = u64::from_le_bytes(bytes[CRC_OFFSET..HEADER_BYTES].try_into().unwrap());
        assert_eq!(stored_crc, u64::from(crc32fast::hash(&bytes[..CRC_OFFSET])));

        let reloaded = cache.try_load(&addr).expect("hit");
        assert_eq!(reloaded.code_size(), loaded.code_size());

        // `instantiation_points` prices each shape count with its own constant, so a hit must serve
        // them in the slots a fresh compile wrote them to.
        let LoadedTemplate::Wasm(reloaded) = &reloaded;
        let LoadedTemplate::Wasm(loaded) = &loaded;
        assert_eq!(reloaded.shape(), loaded.shape());
    }

    #[test]
    fn an_offered_artifact_is_published_by_the_writer() {
        let (loaded, size) = compiled_artifact();
        let dir = TempDir::new().unwrap();
        let cache = WasmModuleCache::open(dir.path(), TEST_CAP_BYTES).unwrap();
        let addr = addr_of_byte(0x11);

        cache.store(&addr, &loaded);
        cache.flush();

        assert!(cache.try_load(&addr).is_some());
        assert_eq!(cache.total_bytes(), size);
    }

    #[test]
    fn the_writer_outlives_a_write_it_cannot_complete() {
        let (loaded, _) = compiled_artifact();
        let dir = TempDir::new().unwrap();
        let cache = WasmModuleCache::open(dir.path(), TEST_CAP_BYTES).unwrap();

        // Nowhere to write, so the first artifact fails somewhere between tempfile and rename.
        fs::remove_dir_all(dir.path()).unwrap();
        cache.store(&addr_of_byte(0x11), &loaded);
        cache.flush();

        fs::create_dir_all(dir.path()).unwrap();
        let addr = addr_of_byte(0x22);
        cache.store(&addr, &loaded);
        cache.flush();

        assert!(
            cache.path_for(&addr).exists(),
            "one artifact that could not be written must not cost every artifact after it",
        );
    }

    #[test]
    fn evicts_the_least_recently_used_artifact_over_the_cap() {
        let (loaded, size) = compiled_artifact();
        let dir = TempDir::new().unwrap();
        // Room for one artifact and change, so the second store must displace the first.
        let cache = WasmModuleCache::open(dir.path(), size + size / 2).unwrap();

        let first = addr_of_byte(0x11);
        let second = addr_of_byte(0x22);
        cache.store(&first, &loaded);
        cache.store(&second, &loaded);
        cache.flush();

        assert!(
            !cache.path_for(&first).exists(),
            "the colder artifact should be evicted"
        );
        assert!(
            cache.path_for(&second).exists(),
            "the artifact just stored should be kept"
        );
        assert_eq!(cache.total_bytes(), size);
    }

    #[test]
    fn a_hit_spares_an_artifact_from_the_next_eviction() {
        let (loaded, size) = compiled_artifact();
        let dir = TempDir::new().unwrap();
        // Room for two artifacts, so the third store evicts whichever went longest unwanted.
        let cache = WasmModuleCache::open(dir.path(), 2 * size + size / 2).unwrap();

        let first = addr_of_byte(0x11);
        let second = addr_of_byte(0x22);
        let third = addr_of_byte(0x33);
        cache.store(&first, &loaded);
        cache.store(&second, &loaded);
        cache.flush();
        cache.try_load(&first).expect("hit");
        cache.store(&third, &loaded);
        cache.flush();

        assert!(cache.path_for(&first).exists(), "the artifact just read should be kept");
        assert!(
            !cache.path_for(&second).exists(),
            "the artifact nobody wanted should go"
        );
        assert!(cache.path_for(&third).exists());
    }

    #[test]
    fn an_artifact_larger_than_the_cap_is_kept() {
        let (loaded, _) = compiled_artifact();
        let dir = TempDir::new().unwrap();
        let cache = WasmModuleCache::open(dir.path(), 1).unwrap();

        let addr = addr_of_byte(0x11);
        cache.store(&addr, &loaded);
        cache.flush();

        assert!(
            cache.path_for(&addr).exists(),
            "evicting the only artifact would recompile it on every call",
        );
    }

    #[test]
    fn reopening_tallies_what_is_already_on_disk() {
        let (loaded, size) = compiled_artifact();
        let dir = TempDir::new().unwrap();
        let first = addr_of_byte(0x11);
        let second = addr_of_byte(0x22);
        {
            let cache = WasmModuleCache::open(dir.path(), TEST_CAP_BYTES).unwrap();
            cache.store(&first, &loaded);
            cache.store(&second, &loaded);
            cache.flush();
        }

        let reopened = WasmModuleCache::open(dir.path(), TEST_CAP_BYTES).unwrap();
        assert_eq!(reopened.total_bytes(), 2 * size);

        // A cap below what the directory holds is applied before the first lookup.
        let shrunk = WasmModuleCache::open(dir.path(), size + size / 2).unwrap();
        assert_eq!(shrunk.total_bytes(), size);
    }

    #[test]
    fn opening_reaps_artifacts_under_a_superseded_fingerprint() {
        let (loaded, size) = compiled_artifact();
        let dir = TempDir::new().unwrap();
        let addr = addr_of_byte(0x11);
        {
            let cache = WasmModuleCache::open(dir.path(), TEST_CAP_BYTES).unwrap();
            cache.store(&addr, &loaded);
            cache.flush();
        }

        // An artifact this build cannot read, left by a run under an older engine config, and a file
        // that is not this cache's at all.
        let superseded = dir.path().join(format!("{}_v0.bin", addr_of_byte(0x22)));
        let unrelated = dir.path().join("notes.txt");
        fs::write(&superseded, b"an older generation").unwrap();
        fs::write(&unrelated, b"not ours").unwrap();

        let cache = WasmModuleCache::open(dir.path(), TEST_CAP_BYTES).unwrap();

        assert!(
            !superseded.exists(),
            "nothing else will ever collect a superseded generation",
        );
        assert!(unrelated.exists(), "a file this cache did not write is left alone");
        assert!(cache.path_for(&addr).exists());
        assert_eq!(cache.total_bytes(), size);
    }

    #[test]
    fn opening_reaps_abandoned_tempfiles_only() {
        let dir = TempDir::new().unwrap();
        let addr = addr_of_byte(0x11);
        let abandoned = dir
            .path()
            .join(format!("{}_{}.bin.tmp.1234.0", addr, ENGINE_FINGERPRINT));
        let in_flight = dir
            .path()
            .join(format!("{}_{}.bin.tmp.5678.0", addr, ENGINE_FINGERPRINT));
        fs::write(&abandoned, b"partial").unwrap();
        fs::write(&in_flight, b"partial").unwrap();

        let aged = SystemTime::now() - (STALE_TEMPFILE_AGE * 2);
        fs::File::options()
            .write(true)
            .open(&abandoned)
            .unwrap()
            .set_times(fs::FileTimes::new().set_accessed(aged).set_modified(aged))
            .unwrap();

        let cache = WasmModuleCache::open(dir.path(), TEST_CAP_BYTES).unwrap();

        assert!(!abandoned.exists(), "a tempfile no store is writing should be reaped");
        assert!(
            in_flight.exists(),
            "a tempfile a store may still be writing should be left"
        );
        assert_eq!(cache.total_bytes(), 0, "tempfiles do not count towards the cap");
    }

    #[test]
    #[cfg(unix)]
    fn a_republished_file_survives_a_stale_unlink() {
        let (loaded, _) = compiled_artifact();
        let dir = TempDir::new().unwrap();
        let cache = WasmModuleCache::open(dir.path(), TEST_CAP_BYTES).unwrap();
        let addr = addr_of_byte(0x11);

        cache.store(&addr, &loaded);
        cache.flush();
        let path = cache.path_for(&addr);
        let stale_identity = fs::metadata(&path).ok().as_ref().and_then(identity_of);

        // A second store publishes a new inode over the same name.
        cache.store(&addr, &loaded);
        cache.flush();

        remove_tracked_file(&path, stale_identity);

        assert!(
            path.exists(),
            "an unlink decided against the replaced file must not land"
        );
        assert!(cache.try_load(&addr).is_some());
    }

    #[test]
    fn fingerprint_mismatch_treated_as_miss() {
        let dir = TempDir::new().unwrap();
        let cache = WasmModuleCache::open(dir.path(), TEST_CAP_BYTES).unwrap();
        let (_store, addr) = make_store();

        // Plant a file under a different fingerprint suffix, after the open that would have reaped it.
        let alt = dir.path().join(format!("{}_v0.bin", addr));
        fs::write(&alt, b"some bytes").unwrap();

        // A lookup names this build's file, which does not exist, and never reads the other.
        assert!(cache.try_load(&addr).is_none());
        assert!(alt.exists());
    }
}
