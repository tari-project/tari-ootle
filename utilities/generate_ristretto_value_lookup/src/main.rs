//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    fs,
    io,
    io::{BufWriter, Write},
    mem::size_of,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use futures::{StreamExt, stream::FuturesOrdered};
use human_bytes::human_bytes;
use tari_crypto::{
    keys::PublicKey,
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
    tari_utilities::ByteArray,
};
use tari_ootle_wallet_crypto::SortedLookupHeader;
use tempfile::{Builder, TempDir};

use crate::cli::Cli;
mod cli;

/// Width of the internal sort key: the first [`KEY_LEN`] bytes of the compressed point read as a big-endian
/// `u64`. The stored `prefix_len` is a truncation of this key, so `prefix_len` cannot exceed [`KEY_LEN`].
const KEY_LEN: usize = size_of::<u64>();

/// Bytes per bucket temp-file entry: the full 8-byte sort key plus the 8-byte value offset. The full key
/// (not just the stored `prefix_len` bytes) is kept so the within-bucket sort orders records exactly as a
/// global sort of all entries would.
const ENTRY_SIZE: usize = KEY_LEN + size_of::<u64>();

/// Approximate total staging memory across all bucket write buffers during generation.
const TOTAL_STAGING_BYTES: usize = 256 * 1024 * 1024;
const MIN_FLUSH_BYTES: usize = 16 * 1024;
const MAX_FLUSH_BYTES: usize = 1024 * 1024;

/// Target size for a single bucket's in-memory sort when choosing the bucket count automatically.
const TARGET_BUCKET_BYTES: u64 = 256 * 1024 * 1024;
const MAX_BUCKET_BITS: u8 = 14;

#[tokio::main]
async fn main() -> io::Result<()> {
    let cli = Cli::init();
    let dest_file = cli.output_file.clone();

    let max = cli
        .max
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "--max is required when generating a table"))?;
    if max < cli.min {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("max ({}) must be >= min ({})", max, cli.min),
        ));
    }

    let jobs = cli
        .jobs
        .unwrap_or_else(|| tokio::runtime::Handle::current().metrics().num_workers());

    generate_sorted(&dest_file, cli.min, max, cli.prefix_len, jobs, cli.bucket_bits).await?;

    println!();
    let metadata = fs::metadata(&dest_file)?;
    println!(
        "Output written to {} ({})",
        dest_file.display(),
        human_bytes(metadata.len() as f64),
    );

    Ok(())
}

/// Generates the sorted prefix-index (`VLK2`) table.
///
/// Small tables are sorted in memory; larger ones use an external bucket sort — EC points are computed in
/// parallel and distributed into per-bucket temp files keyed by the top bits of the sort key, then each
/// bucket is sorted in memory and appended in bucket order. Peak memory for the external path is bounded by
/// the largest bucket, not the table size, so tables larger than RAM can be generated.
///
/// The table is built in a sibling temp file and atomically renamed onto `dest` only once complete, so a
/// failed or interrupted run never truncates or corrupts an existing table.
async fn generate_sorted(
    dest: &Path,
    min: u64,
    max: u64,
    prefix_len: u8,
    num_threads: usize,
    bucket_bits: Option<u8>,
) -> io::Result<()> {
    if !(1..=KEY_LEN as u8).contains(&prefix_len) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("prefix_len must be in 1..={KEY_LEN}"),
        ));
    }

    let value_len = SortedLookupHeader::required_value_len(min, max);
    let header = SortedLookupHeader::new(min, max, prefix_len, value_len);
    let count = header.count();
    let file_size = SortedLookupHeader::SIZE as u64 + count * header.stride() as u64;

    let bucket_bits = match bucket_bits {
        Some(bits) if bits > MAX_BUCKET_BITS => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("bucket_bits must be in 0..={MAX_BUCKET_BITS}"),
            ));
        },
        Some(bits) => bits,
        None => auto_bucket_bits(count),
    };
    let num_buckets = 1usize << bucket_bits;

    println!(
        "Generating sorted prefix-index table from {min} to {max} -> {} ({}). prefix_len={prefix_len}, \
         value_len={value_len}.",
        dest.display(),
        human_bytes(file_size as f64),
    );
    if num_buckets == 1 {
        println!(
            "In-memory sort: ~{} of RAM.\n",
            human_bytes(count as f64 * ENTRY_SIZE as f64),
        );
    } else {
        println!(
            "External sort: {num_buckets} buckets over the top {bucket_bits} key bits; ~{} of temp disk, ~{} of RAM \
             per bucket.\n",
            human_bytes(count as f64 * ENTRY_SIZE as f64),
            human_bytes((count >> bucket_bits) as f64 * ENTRY_SIZE as f64),
        );
    }

    let parent = parent_dir(dest);
    let mut out = Builder::new()
        .prefix(".value_lookup")
        .suffix(".tmp")
        .tempfile_in(&parent)?;
    {
        let mut writer = BufWriter::new(out.as_file_mut());
        header.encode_into(&mut writer)?;
        if num_buckets == 1 {
            sort_in_memory(&mut writer, &header, min, max, num_threads).await?;
        } else {
            sort_external(&mut writer, &header, min, max, num_threads, bucket_bits, &parent).await?;
        }
        writer.flush()?;
    }
    // Atomic rename onto the final path; the previous table (if any) is replaced only once the new one is
    // fully written.
    out.persist(dest).map_err(|e| e.error)?;
    Ok(())
}

/// Sorts the whole table in memory and writes it. Used for tables small enough to sort without spilling to
/// disk (`bucket_bits == 0`).
async fn sort_in_memory<W: Write>(
    writer: &mut W,
    header: &SortedLookupHeader,
    min: u64,
    max: u64,
    num_threads: usize,
) -> io::Result<()> {
    let count = header.count();
    println!("Phase 1/2: generating {count} points on {num_threads} worker threads.");
    let mut entries = Vec::with_capacity(count as usize);
    generate_entries(min, max, num_threads, |chunk| {
        entries.extend_from_slice(chunk);
        Ok(())
    })
    .await?;

    println!("\nPhase 2/2: sorting {count} entries.");
    let timer = Instant::now();
    entries.sort_unstable();
    let mut record = vec![0u8; header.stride()];
    write_records(writer, &entries, header, &mut record)?;
    println!(
        "Sorted and wrote {count} entries in {}.",
        humantime::format_duration(Duration::from_secs(timer.elapsed().as_secs()))
    );
    Ok(())
}

/// Sorts the table via an external bucket sort staged in temp files under `parent`, writing the final records
/// through `writer`.
async fn sort_external<W: Write>(
    writer: &mut W,
    header: &SortedLookupHeader,
    min: u64,
    max: u64,
    num_threads: usize,
    bucket_bits: u8,
    parent: &Path,
) -> io::Result<()> {
    let count = header.count();
    let num_buckets = 1usize << bucket_bits;
    let mut store = BucketStore::create(parent, bucket_bits)?;

    // Phase 1: parallel EC point generation, distributed into bucket temp files.
    println!("Phase 1/2: generating {count} points on {num_threads} worker threads.");
    let timer = Instant::now();
    generate_entries(min, max, num_threads, |chunk| {
        for &(key, offset) in chunk {
            store.push(key, offset)?;
        }
        Ok(())
    })
    .await?;
    store.flush_all()?;
    println!(
        "Distributed into {num_buckets} buckets in {}.",
        humantime::format_duration(Duration::from_secs(timer.elapsed().as_secs()))
    );

    // Phase 2: sort each bucket in memory and append in bucket order. Buckets partition the key space in
    // ascending order, so the concatenation is globally sorted.
    println!("\nPhase 2/2: sorting and writing {count} entries in {num_buckets} bucket(s).");
    let timer = Instant::now();
    let mut record = vec![0u8; header.stride()];
    let mut written = 0u64;
    let mut last_report = 0u64;
    for bucket in 0..num_buckets {
        let mut entries = store.read_bucket(bucket)?;
        entries.sort_unstable();
        write_records(writer, &entries, header, &mut record)?;
        written += entries.len() as u64;
        store.remove_bucket(bucket)?;

        if written - last_report > count / 20 {
            last_report = written;
            println!(
                "{:.1}% ({written}/{count}) sorted and written in {}",
                written as f64 / count as f64 * 100.0,
                humantime::format_duration(Duration::from_secs(timer.elapsed().as_secs())),
            );
        }
    }
    if written != count {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("bucket files contained {written} entries, expected {count}"),
        ));
    }
    println!(
        "Sorted and wrote {count} entries in {}.",
        humantime::format_duration(Duration::from_secs(timer.elapsed().as_secs()))
    );
    Ok(())
}

/// Encodes `entries` as `VLK2` records — `prefix = key.to_be_bytes()[..prefix_len]`, followed by the value
/// offset little-endian in `value_len` bytes — reusing `record` as scratch. `record.len()` must equal the
/// header stride.
fn write_records<W: Write>(
    writer: &mut W,
    entries: &[(u64, u64)],
    header: &SortedLookupHeader,
    record: &mut [u8],
) -> io::Result<()> {
    let prefix_len = header.prefix_len as usize;
    let value_len = header.value_len as usize;
    for &(key, offset) in entries {
        record[..prefix_len].copy_from_slice(&key.to_be_bytes()[..prefix_len]);
        record[prefix_len..].copy_from_slice(&offset.to_le_bytes()[..value_len]);
        writer.write_all(record)?;
    }
    Ok(())
}

/// Computes `(sort_key, value_offset)` for every value in `min..=max` using `num_threads` blocking workers
/// and feeds each generated chunk to `sink`.
///
/// `sort_key` is the first [`KEY_LEN`] bytes of the compressed point read big-endian, so numeric ordering of
/// the key equals lexicographic ordering of the stored prefix bytes.
async fn generate_entries<F>(min: u64, max: u64, num_threads: usize, mut sink: F) -> io::Result<()>
where F: FnMut(&[(u64, u64)]) -> io::Result<()> {
    const CHUNK_SIZE: u64 = 50_000;
    let count = (max - min + 1) as usize;

    let mut chunks = (min..=max).step_by(CHUNK_SIZE as usize).map(|chunk_start| {
        let chunk_end = std::cmp::min(chunk_start + CHUNK_SIZE - 1, max);
        (chunk_start, chunk_end)
    });

    let mut handles = FuturesOrdered::new();
    let mut done = 0usize;
    let timer = Instant::now();
    let mut last_report = 0usize;

    loop {
        while handles.len() < num_threads {
            match chunks.next() {
                Some((chunk_start, chunk_end)) => {
                    handles.push_back(tokio::task::spawn_blocking(move || {
                        let mut out = Vec::with_capacity((chunk_end - chunk_start + 1) as usize);
                        for v in chunk_start..=chunk_end {
                            let pk = RistrettoPublicKey::from_secret_key(&RistrettoSecretKey::from(v));
                            let mut key = [0u8; KEY_LEN];
                            key.copy_from_slice(&pk.as_bytes()[..KEY_LEN]);
                            out.push((u64::from_be_bytes(key), v - min));
                        }
                        out
                    }));
                },
                None => break,
            }
        }

        if handles.is_empty() {
            break;
        }

        let chunk = handles.next().await.expect("handles stream end")?;
        done += chunk.len();
        sink(&chunk)?;

        if done - last_report > count / 20 {
            last_report = done;
            let elapsed = timer.elapsed();
            let eta = Duration::from_secs(
                ((count - done) as f64 / done.max(1) as f64 * elapsed.as_secs_f64()).round() as u64,
            );
            println!(
                "{:.1}% ({done}/{count}) generated in {}, ETA {}",
                done as f64 / count as f64 * 100.0,
                humantime::format_duration(Duration::from_secs(elapsed.as_secs())),
                humantime::format_duration(eta),
            );
        }
    }

    Ok(())
}

/// Picks the number of bucket bits so a single bucket's in-memory sort stays around [`TARGET_BUCKET_BYTES`].
/// Returns 0 (in-memory sort) for tables already within that budget.
fn auto_bucket_bits(count: u64) -> u8 {
    let total_bytes = count.saturating_mul(ENTRY_SIZE as u64);
    let mut bits = 0u8;
    while bits < MAX_BUCKET_BITS && (total_bytes >> bits) > TARGET_BUCKET_BYTES {
        bits += 1;
    }
    bits
}

/// Directory to place sibling temp artifacts in: the output's parent, or the current directory for a bare
/// filename.
fn parent_dir(dest: &Path) -> PathBuf {
    match dest.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// Distributes `(sort_key, value_offset)` entries into per-bucket temp files partitioned by the top
/// `bucket_bits` bits of the key. Writes are staged in per-bucket memory buffers and appended to the bucket
/// file once a buffer reaches the flush threshold, so at most one temp file is open at a time.
///
/// Only entered with `bucket_bits >= 1`. Backing files live in a private temp directory removed when the
/// store is dropped, so an interrupted run leaves no state behind.
struct BucketStore {
    dir: TempDir,
    bucket_bits: u8,
    staged: Vec<Vec<u8>>,
    flush_threshold: usize,
}

impl BucketStore {
    fn create(parent: &Path, bucket_bits: u8) -> io::Result<Self> {
        let dir = Builder::new().prefix(".value_lookup_buckets").tempdir_in(parent)?;
        let num_buckets = 1usize << bucket_bits;
        let flush_threshold = (TOTAL_STAGING_BYTES / num_buckets).clamp(MIN_FLUSH_BYTES, MAX_FLUSH_BYTES);
        Ok(Self {
            dir,
            bucket_bits,
            staged: vec![Vec::with_capacity(flush_threshold); num_buckets],
            flush_threshold,
        })
    }

    fn bucket_of(&self, key: u64) -> usize {
        (key >> (64 - u32::from(self.bucket_bits))) as usize
    }

    fn bucket_path(&self, bucket: usize) -> PathBuf {
        self.dir.path().join(format!("bucket_{bucket:05}.bin"))
    }

    fn push(&mut self, key: u64, offset: u64) -> io::Result<()> {
        let bucket = self.bucket_of(key);
        let buf = &mut self.staged[bucket];
        buf.extend_from_slice(&key.to_le_bytes());
        buf.extend_from_slice(&offset.to_le_bytes());
        if buf.len() >= self.flush_threshold {
            self.flush_bucket(bucket)?;
        }
        Ok(())
    }

    fn flush_bucket(&mut self, bucket: usize) -> io::Result<()> {
        if self.staged[bucket].is_empty() {
            return Ok(());
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.bucket_path(bucket))?;
        file.write_all(&self.staged[bucket])?;
        self.staged[bucket].clear();
        Ok(())
    }

    fn flush_all(&mut self) -> io::Result<()> {
        for bucket in 0..self.staged.len() {
            self.flush_bucket(bucket)?;
        }
        Ok(())
    }

    /// Reads all entries of a bucket into memory. A bucket that never received an entry has no file and
    /// yields an empty vec.
    fn read_bucket(&self, bucket: usize) -> io::Result<Vec<(u64, u64)>> {
        let path = self.bucket_path(bucket);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err),
        };
        if bytes.len() % ENTRY_SIZE != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("bucket file {} contains a partial entry", path.display()),
            ));
        }
        let (chunks, _) = bytes.as_chunks::<ENTRY_SIZE>();
        let entries = chunks
            .iter()
            .map(|c| {
                (
                    u64::from_le_bytes(c[..8].try_into().expect("chunk is ENTRY_SIZE bytes")),
                    u64::from_le_bytes(c[8..].try_into().expect("chunk is ENTRY_SIZE bytes")),
                )
            })
            .collect();
        Ok(entries)
    }

    fn remove_bucket(&self, bucket: usize) -> io::Result<()> {
        match fs::remove_file(self.bucket_path(bucket)) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use tari_ootle_wallet_crypto::SortedPrefixFileLookup;
    use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

    use super::*;

    fn point_bytes(value: u64) -> RistrettoPublicKeyBytes {
        let pk = RistrettoPublicKey::from_secret_key(&RistrettoSecretKey::from(value));
        RistrettoPublicKeyBytes::from_bytes(pk.as_bytes()).unwrap()
    }

    async fn assert_round_trips(min: u64, max: u64, bucket_bits: Option<u8>) {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("value_lookup.bin");
        generate_sorted(&dest, min, max, 8, 4, bucket_bits).await.unwrap();

        let file = File::open(&dest).unwrap();
        // Safety: the file is not modified while mapped.
        let lookup = unsafe { SortedPrefixFileLookup::load(&file).unwrap() };
        assert_eq!(lookup.range(), min..=max);
        for v in min..=max {
            assert_eq!(lookup.find_value(&point_bytes(v)), Some(v), "failed at {v}");
        }
        assert_eq!(lookup.find_value(&point_bytes(min.wrapping_sub(1))), None);
        assert_eq!(lookup.find_value(&point_bytes(max + 1)), None);
    }

    #[tokio::test]
    async fn external_sort_round_trips_through_the_reader() {
        // 64 buckets spread the entries across many temp files.
        assert_round_trips(1000, 5000, Some(6)).await;
    }

    #[tokio::test]
    async fn in_memory_sort_round_trips_through_the_reader() {
        assert_round_trips(1000, 5000, Some(0)).await;
    }
}
