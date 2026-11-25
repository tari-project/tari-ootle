//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Facts about the machine under test, recorded alongside every measurement.
//!
//! A throughput number is only interpretable next to the machine that produced it, and a run is
//! only comparable to another run whose host is described the same way — so this is captured
//! unconditionally, including in `--json`, and is what `--compare` keys its diff against.
//!
//! Everything here is best-effort: the fields come from `/proc`, and a host that does not provide
//! them reports `None` rather than failing the run. A missing CPU model never invalidates a timing.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    thread,
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Host {
    /// Operator-supplied name for this machine, so a comparison of two reports says which is which.
    pub label: String,
    pub os: String,
    pub arch: String,
    pub cpu_model: Option<String>,
    /// Schedulable threads. Consensus executes a block's transactions serially, so this is the
    /// wrong number to size against — it is recorded to explain the single-core figures, not to
    /// substitute for them.
    pub logical_cpus: usize,
    /// Physical cores, where `/proc/cpuinfo` reports them. Below `logical_cpus` means SMT is on,
    /// and SMT siblings do not add single-thread speed.
    pub physical_cpus: Option<usize>,
    pub cpu_max_mhz: Option<f64>,
    pub mem_total_bytes: Option<u64>,
    pub mem_available_bytes: Option<u64>,
    /// Peak resident set of the benchmark process itself. Not the node's working set — the
    /// benchmark holds one engine and no consensus, storage or networking state — but a floor
    /// under it, and a check that the engine's own footprint is what it should be.
    pub peak_rss_bytes: Option<u64>,
    /// A debug build's engine runs several times slower than a release build's, which would make
    /// every projection in the report meaningless. Recorded so a report produced by the wrong
    /// binary is identifiable after the fact, and warned about at run time.
    pub debug_assertions: bool,
}

impl Host {
    pub fn detect(label: String) -> Self {
        let cpuinfo = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
        Self {
            label,
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            cpu_model: field(&cpuinfo, "model name"),
            logical_cpus: thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
            physical_cpus: parse_physical_cpus(&cpuinfo),
            cpu_max_mhz: read_max_mhz(),
            mem_total_bytes: meminfo_bytes("MemTotal"),
            mem_available_bytes: meminfo_bytes("MemAvailable"),
            peak_rss_bytes: None,
            debug_assertions: cfg!(debug_assertions),
        }
    }

    /// Samples the high-water mark. Called at the end of the run, once every phase has allocated
    /// whatever it is going to.
    pub fn record_peak_rss(&mut self) {
        self.peak_rss_bytes = status_field_bytes("VmHWM");
    }

    /// True when SMT is enabled, i.e. the logical count overstates how many transactions-worth of
    /// independent execution the machine really has.
    pub fn smt_enabled(&self) -> bool {
        self.physical_cpus.is_some_and(|physical| physical < self.logical_cpus)
    }
}

/// First value of a `key : value` line in `/proc/cpuinfo`. Every core repeats the same model, so
/// the first is representative on the homogeneous hosts this runs on.
fn field(cpuinfo: &str, key: &str) -> Option<String> {
    cpuinfo
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(k, _)| k.trim() == key)
        .map(|(_, value)| value.trim().to_string())
}

/// Counts total physical cores across all sockets in `/proc/cpuinfo`.
///
/// `cpu cores` in `/proc/cpuinfo` reports cores per physical socket (`physical id`), not the
/// machine total. Prefer counting distinct `(physical id, core id)` pairs across processor blocks,
/// falling back to summing `cpu cores` across distinct `physical id`s or a single `cpu cores` entry.
fn parse_physical_cpus(cpuinfo: &str) -> Option<usize> {
    let mut socket_cores: BTreeSet<(usize, usize)> = BTreeSet::new();
    let mut cores_per_socket: BTreeMap<usize, usize> = BTreeMap::new();
    let mut fallback_cpu_cores: Option<usize> = None;

    let mut current_socket: Option<usize> = None;
    let mut current_core: Option<usize> = None;
    let mut current_cpu_cores: Option<usize> = None;

    let mut flush_block = |socket: &mut Option<usize>, core: &mut Option<usize>, cpu_cores: &mut Option<usize>| {
        if let (Some(s), Some(c)) = (*socket, *core) {
            socket_cores.insert((s, c));
        }
        if let (Some(s), Some(cores)) = (*socket, *cpu_cores) {
            cores_per_socket.entry(s).or_insert(cores);
        }
        if fallback_cpu_cores.is_none() {
            fallback_cpu_cores = *cpu_cores;
        }
        *socket = None;
        *core = None;
        *cpu_cores = None;
    };

    for line in cpuinfo.lines() {
        if line.trim().is_empty() {
            flush_block(&mut current_socket, &mut current_core, &mut current_cpu_cores);
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "processor" => {
                flush_block(&mut current_socket, &mut current_core, &mut current_cpu_cores);
            },
            "physical id" => current_socket = value.parse().ok(),
            "core id" => current_core = value.parse().ok(),
            "cpu cores" => current_cpu_cores = value.parse().ok(),
            _ => {},
        }
    }
    flush_block(&mut current_socket, &mut current_core, &mut current_cpu_cores);

    if !socket_cores.is_empty() {
        Some(socket_cores.len())
    } else if !cores_per_socket.is_empty() {
        Some(cores_per_socket.values().sum::<usize>())
    } else {
        fallback_cpu_cores.filter(|&c| c > 0)
    }
}

/// Maximum non-turbo frequency from cpufreq, in MHz. Absent on hosts without the driver, and on
/// most VMs — which is itself informative, since a guest usually cannot see or pin its own clock.
fn read_max_mhz() -> Option<f64> {
    let khz = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq").ok()?;
    khz.trim().parse::<f64>().ok().map(|khz| khz / 1000.0)
}

fn meminfo_bytes(key: &str) -> Option<u64> {
    let meminfo = fs::read_to_string("/proc/meminfo").ok()?;
    kib_line(&meminfo, key)
}

fn status_field_bytes(key: &str) -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    kib_line(&status, key)
}

/// Parses a `Key:   1234 kB` line, the shared shape of `/proc/meminfo` and `/proc/self/status`.
fn kib_line(text: &str, key: &str) -> Option<u64> {
    text.lines()
        .find(|line| line.starts_with(key) && line[key.len()..].starts_with(':'))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|kib| kib.parse::<u64>().ok())
        .map(|kib| kib * 1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_physical_cpus_counts_across_sockets_and_smt_siblings() {
        let single_socket_smt = "\
processor\t: 0\nphysical id\t: 0\ncore id\t\t: 0\ncpu cores\t: 2\n\n\
processor\t: 1\nphysical id\t: 0\ncore id\t\t: 1\ncpu cores\t: 2\n\n\
processor\t: 2\nphysical id\t: 0\ncore id\t\t: 0\ncpu cores\t: 2\n\n\
processor\t: 3\nphysical id\t: 0\ncore id\t\t: 1\ncpu cores\t: 2\n";
        assert_eq!(parse_physical_cpus(single_socket_smt), Some(2));

        let multi_socket_vm = "\
processor\t: 0\nphysical id\t: 0\ncore id\t\t: 0\ncpu cores\t: 1\n\n\
processor\t: 1\nphysical id\t: 1\ncore id\t\t: 0\ncpu cores\t: 1\n\n\
processor\t: 2\nphysical id\t: 2\ncore id\t\t: 0\ncpu cores\t: 1\n\n\
processor\t: 3\nphysical id\t: 3\ncore id\t\t: 0\ncpu cores\t: 1\n";
        assert_eq!(parse_physical_cpus(multi_socket_vm), Some(4));

        let dual_socket_without_core_id = "\
processor\t: 0\nphysical id\t: 0\ncpu cores\t: 8\n\n\
processor\t: 8\nphysical id\t: 1\ncpu cores\t: 8\n";
        assert_eq!(parse_physical_cpus(dual_socket_without_core_id), Some(16));
    }
}
