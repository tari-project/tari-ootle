//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Measures how transaction execution throughput scales with worker threads.
//!
//! Each transaction executes against its own immutable snapshot of the same state, exactly the
//! isolation a parallel block executor would give independent transactions, so the only coupling
//! between workers is what the process itself imposes: the shared compiled-module cache, the
//! allocator, and memory bandwidth. The reported speedup is therefore the ceiling a parallel
//! scheduler can reach on this machine for conflict-free transactions.
//!
//! Two workloads bracket the traffic mix:
//! * `transfer` — the canonical account-to-account transfer: engine/native dominated, the shape that fills real blocks.
//! * `grind` — a compute-bound WASM loop: pure metered execution, the shape the block execution-point budget is
//!   calibrated against.

use std::time::Instant;

use tari_ootle_transaction::{Transaction, args};
use tari_template_lib::types::{NonFungibleAddress, TemplateAddress, constants::TARI_TOKEN};
use tari_template_test_tooling::{Package, SnapshotExecutor, TemplateTest};

const MAX_FEE: u64 = 60_000_000;

const COMPUTE_BENCH_WASM: &[u8] = include_bytes!("../../compiled/compute_bench.wasm");
const COMPUTE_BENCH_ADDRESS: TemplateAddress = TemplateAddress::from_array([0xBE; 32]);

const GRIND_ROUNDS: u64 = 10_000;

/// Aim for roughly this long a single-threaded pass; the same transaction count is then reused for
/// every thread count so all runs do identical work.
const TARGET_SINGLE_THREAD_SECS: f64 = 5.0;
const MIN_TXS: usize = 64;
const MAX_TXS: usize = 4096;
const REPEATS: usize = 3;

fn loadavg() -> String {
    std::fs::read_to_string("/proc/loadavg")
        .map(|s| s.split_whitespace().take(3).collect::<Vec<_>>().join(" "))
        .unwrap_or_else(|_| "unknown".into())
}

fn main() -> anyhow::Result<()> {
    let cpus = std::thread::available_parallelism()?.get();
    // PS_THREADS ("1,8") and PS_WORKLOAD ("transfer"/"grind") narrow a run for profiling.
    let thread_counts: Vec<usize> = std::env::var("PS_THREADS")
        .ok()
        .map(|s| {
            s.split(',')
                .filter_map(|t| t.trim().parse::<usize>().ok())
                .filter(|&k| k > 0)
                .collect()
        })
        .unwrap_or_else(|| {
            [1, 2, 4, 8, 16, 24, 32, 48, 64]
                .into_iter()
                .filter(|&k| k <= cpus)
                .collect()
        });
    if thread_counts.is_empty() {
        anyhow::bail!("no valid (> 0) thread counts specified in PS_THREADS");
    }
    let workload = std::env::var("PS_WORKLOAD").unwrap_or_else(|_| "both".into());

    println!("host: {cpus} cpus, loadavg at start: {}", loadavg());

    let mut builder = Package::builder();
    builder.add_all_builtin_templates();
    builder
        .add_template_from_code(COMPUTE_BENCH_ADDRESS, COMPUTE_BENCH_WASM)
        .map_err(|e| anyhow::anyhow!("embedded compute_bench template failed to load: {e}"))?;
    let package = builder.build();

    let mut test = TemplateTest::from_package(package);
    test.bootstrap_state();
    test.enable_fees();

    let (sender, sender_proof, sender_key) = test.create_funded_account();
    let (receiver, _, _) = test.create_empty_account();

    let executor = test.snapshot_executor();

    let build_transfer = |test: &TemplateTest| {
        test.transaction()
            .with_unversioned_inputs([sender, receiver])
            .pay_fee_from_component(sender, MAX_FEE)
            .call_method(sender, "withdraw", args![TARI_TOKEN, 1])
            .put_last_instruction_output_on_workspace("transferred")
            .call_method(receiver, "deposit", args![Workspace("transferred")])
            .build_and_seal(&sender_key)
    };
    let build_grind = |test: &TemplateTest| {
        test.transaction()
            .pay_fee_from_component(sender, MAX_FEE)
            .call_function(COMPUTE_BENCH_ADDRESS, "grind", args![GRIND_ROUNDS])
            .build_and_seal(&sender_key)
    };

    if workload == "instantiate" {
        run_instantiate_scaling(&test, &thread_counts)?;
    } else {
        if workload != "grind" {
            run_workload(
                "transfer",
                &test,
                &executor,
                &sender_proof,
                &thread_counts,
                build_transfer,
            )?;
        }
        if workload != "transfer" {
            run_workload("grind", &test, &executor, &sender_proof, &thread_counts, build_grind)?;
        }
    }

    println!("\nloadavg at end: {}", loadavg());
    Ok(())
}

fn run_workload(
    name: &str,
    test: &TemplateTest,
    executor: &SnapshotExecutor,
    proof: &NonFungibleAddress,
    thread_counts: &[usize],
    build: impl Fn(&TemplateTest) -> Transaction,
) -> anyhow::Result<()> {
    // Warm up template lazy-initialisation, then calibrate the per-transaction cost to size the
    // batch.
    execute_one(executor, build(test), proof)?;
    let calib_started = Instant::now();
    for _ in 0..5 {
        execute_one(executor, build(test), proof)?;
    }
    let per_tx_secs = calib_started.elapsed().as_secs_f64() / 5.0;
    let count = ((TARGET_SINGLE_THREAD_SECS / per_tx_secs) as usize).clamp(MIN_TXS, MAX_TXS);

    let transactions: Vec<Transaction> = (0..count).map(|_| build(test)).collect();

    println!(
        "\n== {name}: {count} txs, ~{:.2} ms/tx single-threaded, {REPEATS} repeats (min taken) ==",
        per_tx_secs * 1000.0
    );
    println!(
        "{:>7} {:>10} {:>10} {:>9} {:>11} {:>16}",
        "threads", "wall s", "tx/s", "speedup", "efficiency", "loadavg before"
    );

    let mut base_tx_per_sec = 0.0;
    for &k in thread_counts {
        let load_before = loadavg();
        let mut best_secs = f64::MAX;
        for _ in 0..REPEATS {
            let secs = run_pass(executor, &transactions, proof, k)?;
            best_secs = best_secs.min(secs);
        }
        let tx_per_sec = transactions.len() as f64 / best_secs;
        if k == thread_counts[0] {
            base_tx_per_sec = tx_per_sec;
        }
        let speedup = tx_per_sec / base_tx_per_sec;
        println!(
            "{:>7} {:>10.3} {:>10.1} {:>8.2}x {:>10.1}% {:>16}",
            k,
            best_secs,
            tx_per_sec,
            speedup,
            100.0 * speedup / k as f64,
            load_before
        );
    }
    Ok(())
}

/// Bare wasmer `Store` + `Instance` creation for the Account template — no engine, no execution.
/// Isolates the per-invocation instantiation cost (memory mmap, import resolution, trampolines)
/// from everything else the executor does, to attribute the transfer workload's scaling ceiling.
fn run_instantiate_scaling(test: &TemplateTest, thread_counts: &[usize]) -> anyhow::Result<()> {
    use wasmer::{Extern, Function, Imports, Instance, Value};

    let module = test.get_module("Account");
    const COUNT: usize = 20_000;

    let instantiate = |_: usize| -> anyhow::Result<()> {
        let mut store = module.create_store();
        let mut imports = Imports::new();
        for import in module.wasm_module().imports() {
            if let wasmer::ExternType::Function(ft) = import.ty() {
                let results: Vec<Value> = ft
                    .results()
                    .iter()
                    .map(|t| match t {
                        wasmer::Type::I32 => Value::I32(0),
                        wasmer::Type::I64 => Value::I64(0),
                        wasmer::Type::F32 => Value::F32(0.0),
                        wasmer::Type::F64 => Value::F64(0.0),
                        other => panic!("unsupported stub result type {other:?}"),
                    })
                    .collect();
                let f = Function::new(&mut store, ft, move |_args| Ok(results.clone()));
                imports.define(import.module(), import.name(), Extern::Function(f));
            }
        }
        let instance = Instance::new(&mut store, module.wasm_module(), &imports)?;
        std::hint::black_box(&instance);
        Ok(())
    };

    instantiate(0)?;
    println!("\n== instantiate: {COUNT} bare Store+Instance creations of the Account template, {REPEATS} repeats ==");
    println!(
        "{:>7} {:>10} {:>12} {:>9} {:>11} {:>16}",
        "threads", "wall s", "inst/s", "speedup", "efficiency", "loadavg before"
    );

    let mut base_per_sec = 0.0;
    for &k in thread_counts {
        let load_before = loadavg();
        let mut best_secs = f64::MAX;
        for _ in 0..REPEATS {
            let chunk_size = COUNT.div_ceil(k);
            let started = Instant::now();
            let result: anyhow::Result<()> = std::thread::scope(|scope| {
                let handles: Vec<_> = (0..k)
                    .map(|w| {
                        let n = chunk_size.min(COUNT - (w * chunk_size).min(COUNT));
                        scope.spawn(move || -> anyhow::Result<()> {
                            for i in 0..n {
                                instantiate(i)?;
                            }
                            Ok(())
                        })
                    })
                    .collect();
                for handle in handles {
                    handle.join().expect("worker panicked")?;
                }
                Ok(())
            });
            result?;
            best_secs = best_secs.min(started.elapsed().as_secs_f64());
        }
        let per_sec = COUNT as f64 / best_secs;
        if k == thread_counts[0] {
            base_per_sec = per_sec;
        }
        let speedup = per_sec / base_per_sec;
        println!(
            "{:>7} {:>10.3} {:>12.1} {:>8.2}x {:>10.1}% {:>16}",
            k,
            best_secs,
            per_sec,
            speedup,
            100.0 * speedup / k as f64,
            load_before
        );
    }
    Ok(())
}

/// One timed pass: the transaction list split into `k` contiguous chunks, one worker per chunk.
fn run_pass(
    executor: &SnapshotExecutor,
    transactions: &[Transaction],
    proof: &NonFungibleAddress,
    k: usize,
) -> anyhow::Result<f64> {
    let chunk_size = transactions.len().div_ceil(k);
    let started = Instant::now();
    let result: anyhow::Result<()> = std::thread::scope(|scope| {
        let handles: Vec<_> = transactions
            .chunks(chunk_size)
            .map(|chunk| {
                scope.spawn(move || -> anyhow::Result<()> {
                    for transaction in chunk {
                        execute_one(executor, transaction.clone(), proof)?;
                    }
                    Ok(())
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("worker panicked")?;
        }
        Ok(())
    });
    result?;
    Ok(started.elapsed().as_secs_f64())
}

fn execute_one(
    executor: &SnapshotExecutor,
    transaction: Transaction,
    proof: &NonFungibleAddress,
) -> anyhow::Result<()> {
    let result = executor.execute(transaction, vec![proof.clone()])?;
    if let Some(reason) = result.finalize.any_reject() {
        anyhow::bail!("benchmark transaction was rejected: {reason}");
    }
    Ok(())
}
