//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    fs,
    io,
    io::{Write, stdout},
    time::{Duration, Instant},
};

use futures::{StreamExt, stream::FuturesOrdered};
use human_bytes::human_bytes;
use tari_crypto::{
    keys::PublicKey,
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
    tari_utilities::ByteArray,
};
use tari_ootle_wallet_crypto::{LookupHeader, ValueLookupTable};

use crate::cli::Cli;
mod cli;

#[tokio::main]
async fn main() -> io::Result<()> {
    let cli = Cli::init();
    let dest_file = cli.output_file;

    if cli.validate {
        println!(
            "Validating Ristretto value lookup table at {}. NOTE: this will probably take hours depending on the \
             value range of the file.",
            dest_file.display()
        );
        let timer = Instant::now();
        let metadata = fs::metadata(&dest_file)?;
        let file = fs::File::open(&dest_file)?;
        // SAFETY: We assume the file will not be modified while mapped. Although not enforced (e.g. locks,
        // permissions and other platform specific mechanisms), this is a reasonable assumption for most scenarios.
        let mut lookup = unsafe { tari_ootle_wallet_crypto::MMapValueLookup::load(&file) }?;

        let expected_size = (lookup.range().end() - lookup.range().start() + 1) * 32 + LookupHeader::SIZE as u64;
        if metadata.len() != expected_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "File size mismatch. Expected {} bytes but found {} bytes.",
                    expected_size,
                    metadata.len()
                ),
            ));
        }

        println!(
            "✅ File size OK - header range {} to {}.",
            lookup.range().start(),
            lookup.range().end()
        );

        for v in lookup.range() {
            let pk_bytes = lookup.lookup(v)?.ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Value {} not found in lookup table.", v),
                )
            })?;
            let expected_pk = RistrettoPublicKey::from_secret_key(&RistrettoSecretKey::from(v));
            if pk_bytes != expected_pk.as_bytes() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Public key mismatch for value {}.", v),
                ));
            }
        }
        let elapsed = timer.elapsed();
        println!(
            "Validation completed successfully in {}.",
            humantime::format_duration(elapsed)
        );
        return Ok(());
    }

    let file_size = (cli.max - cli.min + 1) * 32 + LookupHeader::SIZE as u64;
    println!(
        "Generating Ristretto value lookup table from {} to {} and writing to {} ({}).",
        cli.min,
        cli.max,
        dest_file.display(),
        human_bytes(file_size as f64),
    );

    println!();

    let writer = fs::File::create(&dest_file)?;

    // Determine number of workers
    let jobs = cli
        .jobs
        .unwrap_or_else(|| tokio::runtime::Handle::current().metrics().num_workers());
    write_output_async(writer, cli.min, cli.max, jobs).await?;

    println!();

    let metadata = fs::metadata(&dest_file)?;

    println!(
        "Output written to {} ({})",
        dest_file.display(),
        human_bytes(metadata.len() as f64),
    );

    Ok(())
}

async fn write_output_async<W: io::Write>(mut writer: W, min: u64, max: u64, num_threads: usize) -> io::Result<()> {
    LookupHeader::new(min, max).encode_into(&mut writer)?;

    println!(
        "Using {} worker threads to generate Ristretto public keys.",
        num_threads
    );

    const CHUNK_SIZE: usize = 10_000; // 320 KB per thread, because of the result ordering, making the chunk size larger actually degrades performance

    let timer = Instant::now();

    let mut chunks = (min..=max)
        .step_by(CHUNK_SIZE)
        .map(|chunk_start| {
            let chunk_end = std::cmp::min(chunk_start + CHUNK_SIZE as u64 - 1, max);
            (chunk_start, chunk_end)
        })
        .enumerate();

    // Pre-allocate scratch pad for each thread
    let mut scratch_pad = vec![Some(Vec::with_capacity(CHUNK_SIZE)); num_threads];

    let mut handles = FuturesOrdered::new();
    let mut count = 0;
    let mut dot_count = 0;
    let mut line_count = 1;

    loop {
        while handles.len() < num_threads {
            if let Some((i, (chunk_start, chunk_end))) = chunks.by_ref().next() {
                let mut results = scratch_pad[i % num_threads].take().unwrap();
                // Spawn tasks for each chunk
                let handle = tokio::task::spawn_blocking(move || {
                    for v in chunk_start..=chunk_end {
                        let mut buf = [0u8; 32];
                        let pk = RistrettoPublicKey::from_secret_key(&RistrettoSecretKey::from(v));
                        buf.copy_from_slice(pk.as_bytes());
                        results.push(buf);
                    }
                    (i % num_threads, results)
                });

                handles.push_back(handle);
            } else {
                break;
            }
        }

        if handles.is_empty() {
            break;
        }

        let (i, mut results) = handles.next().await.expect("handles stream end")?;
        for pk_bytes in &results {
            count += 1;
            writer.write_all(pk_bytes)?;
        }
        results.clear();
        scratch_pad[i].replace(results);

        if count % 10000 == 0 {
            print!(".");
            dot_count += 1;
            stdout().flush()?;
        }

        if dot_count == 80 {
            dot_count = 0;
            line_count += 1;
            println!();
        }

        if line_count % 5 == 0 {
            line_count = 1;
            let completed = count;
            let elapsed = timer.elapsed();
            let est_time = Duration::from_secs(
                ((max - min + 1 - completed) as f64 / completed as f64 * elapsed.as_secs_f64()).round() as u64,
            );
            println!(
                "{:.1}% ETA: {}. {}/{} values generated in {}",
                (completed as f64 / (max - min + 1) as f64) * 100.0,
                humantime::format_duration(est_time),
                completed,
                max - min,
                humantime::format_duration(Duration::from_secs(elapsed.as_secs()))
            );
        }
    }

    println!();
    println!(
        "Completed generation of Ristretto value lookup table from {} to {} in {}",
        min,
        max,
        humantime::format_duration(Duration::from_secs(timer.elapsed().as_secs()))
    );

    Ok(())
}
