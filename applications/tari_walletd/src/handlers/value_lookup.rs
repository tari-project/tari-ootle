// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::{fs, ops::RangeInclusive, path::Path};

use anyhow::anyhow;
use log::{info, warn};
use tari_crypto::ristretto::RistrettoSecretKey;
use tari_engine_types::crypto::ElgamalVerifiableBalanceBytes;
use tari_ootle_wallet_crypto::{GenerateValueLookup, SortedPrefixFileLookup};
use tari_ootle_wallet_sdk::apis::viewable_balance::ViewableBalanceApi;
use tari_ootle_walletd_client::types::{StealthUtxosGetValueLookupInfoResponse, ValueScanCoverage};
use tokio::task::AbortHandle;

const LOG_TARGET: &str = "tari::ootle::walletd::handlers::value_lookup";

/// Widest span of candidate values the on-the-fly scan will cover when no lookup file is configured,
/// and the span used when the caller names no maximum.
///
/// Without a file each candidate costs a `v·G` scalar multiplication and the scan cannot be
/// interrupted once it has started, so this is the CPU one request is allowed to spend rather than a
/// suggestion — a caller naming a wider range gets the ceiling, not the range it asked for. Ten
/// million candidates covers 0-10 tTARI and completes in minutes; wider coverage is what a generated
/// lookup file is for (`utilities/generate_ristretto_value_lookup`), and with one configured the
/// search is a binary search and this ceiling does not apply.
pub(crate) const MAX_NO_FILE_SCAN_CANDIDATES: u64 = 10_000_000;

/// A caller's expected-value bounds, resolved against policy by [`ValueRangeRequest::resolve`].
///
/// The two fields answer different questions, which is why they are separate. `scan_range` is the
/// work an unindexed recovery will do, and is what one request is allowed to spend. `requested_max`
/// is what the caller said it expected, kept unclamped so that a configured lookup file which does
/// not cover it can say so.
pub(crate) struct ValueRangeRequest {
    scan_range: RangeInclusive<u64>,
    requested_max: Option<u64>,
}

impl ValueRangeRequest {
    /// Clamps the caller's bounds to the [`MAX_NO_FILE_SCAN_CANDIDATES`] candidates one request may
    /// scan.
    ///
    /// A minimum above the maximum resolves to the single-candidate range at the minimum, so the
    /// range the no-file scan logs reads coherently. An empty range would recover the same `None`s.
    pub fn resolve(min_expected: Option<u64>, max_expected: Option<u64>) -> Self {
        let min = min_expected.unwrap_or(0);
        let ceiling = min.saturating_add(MAX_NO_FILE_SCAN_CANDIDATES);
        Self {
            scan_range: min..=max_expected.unwrap_or(ceiling).clamp(min, ceiling),
            requested_max: max_expected,
        }
    }

    /// Reports `searched` back to the caller, flagging it as clamped when it covers less than the
    /// caller asked for at either end.
    ///
    /// Both ends matter, and a lookup file is where they differ: a file covering `1000..=2000` answers
    /// a caller asking from 0 without ever looking below 1000, so a `None` there is "not searched"
    /// exactly as it is above the file's top. `scan_range`'s start is the minimum the caller named, or
    /// 0 when it named none.
    fn coverage(&self, searched: &RangeInclusive<u64>) -> ValueScanCoverage {
        let below = *searched.start() > *self.scan_range.start();
        let above = self.requested_max.is_some_and(|max| max > *searched.end());
        ValueScanCoverage {
            min: *searched.start(),
            max: *searched.end(),
            clamped: below || above,
        }
    }
}

/// One recovered value per proof, in the order the proofs were given, and what the search covered.
///
/// The coverage travels with the balances because a `None` is ambiguous without it: the value may be
/// absent, or it may lie outside what this search reached.
pub(crate) struct BalanceRecovery {
    pub balances: Vec<Option<u64>>,
    pub searched: ValueScanCoverage,
}

/// Aborts a blocking balance-recovery task when the request that spawned it is dropped.
///
/// `JoinHandle::abort` on a `spawn_blocking` task only cancels a closure that has not started yet, so
/// this returns a queued scan's thread to the pool but cannot stop one already running. That is why
/// the scan range and the number of balances are bounded before the task is spawned: the bound is
/// what ends the work.
pub(crate) struct AbortQueuedOnDropGuard(AbortHandle);

impl AbortQueuedOnDropGuard {
    pub fn new(handle: AbortHandle) -> Self {
        Self(handle)
    }
}

impl Drop for AbortQueuedOnDropGuard {
    fn drop(&mut self) {
        if !self.0.is_finished() {
            info!(target: LOG_TARGET, "Request abandoned; cancelling the balance lookup task if it has not started");
            self.0.abort();
        }
    }
}

/// Recovers the plaintext balances behind `proofs` by reverse-searching a value lookup.
///
/// With a lookup file configured, a [`SortedPrefixFileLookup`] is searched by O(log n) binary search per balance. The
/// whole file is searched cheaply, so `range` only hints at the expected coverage rather than
/// bounding the search. A balance whose value is not in the file is reported as `None`.
///
/// Without a lookup file, a [`GenerateValueLookup`] recovers the balances by computing `v·G` over
/// `range`'s scan range, which is very slow and is why that range is bounded.
///
/// This performs blocking CPU/IO work and must be called from a blocking context.
pub(crate) fn brute_force_viewable_balances(
    api: &ViewableBalanceApi,
    lookup_file: Option<&Path>,
    secret_view_key: &RistrettoSecretKey,
    proofs: &[ElgamalVerifiableBalanceBytes],
    range: ValueRangeRequest,
) -> anyhow::Result<BalanceRecovery> {
    let Some(path) = lookup_file else {
        warn!(
            target: LOG_TARGET,
            "No value lookup table file configured. Recovering balances by on-the-fly generation over {}-{}; this \
             may be extremely slow.",
            range.scan_range.start(),
            range.scan_range.end(),
        );
        let searched = range.coverage(&range.scan_range);
        let lookup = GenerateValueLookup::new(range.scan_range.clone());
        return Ok(BalanceRecovery {
            balances: api.try_decrypt_commitment_balances(secret_view_key, proofs.iter(), &lookup)?,
            searched,
        });
    };

    let file =
        fs::File::open(path).map_err(|e| anyhow!("Unable to load value lookup file '{}': {e}", path.display()))?;
    // SAFETY: We assume the file will not be modified while mapped. Although not enforced (e.g. locks, permissions
    // and other platform specific mechanisms), this is a reasonable assumption for most scenarios.
    let lookup = unsafe { SortedPrefixFileLookup::load(&file) }?;
    info!(
        target: LOG_TARGET,
        "Using value lookup table '{}' ({}-{}) for reverse balance lookup",
        path.display(),
        lookup.range().start(),
        lookup.range().end(),
    );

    // A requested maximum above the file's coverage means high-value outputs simply cannot be found; surface it.
    if let Some(max) = range.requested_max &&
        max > *lookup.range().end()
    {
        warn!(
            target: LOG_TARGET,
            "Requested maximum value {max} exceeds the lookup file coverage {}-{}; values above {} cannot be found \
             and require a larger lookup file.",
            lookup.range().start(),
            lookup.range().end(),
            lookup.range().end(),
        );
    }

    let results = api.try_decrypt_commitment_balances(secret_view_key, proofs.iter(), &lookup)?;

    // A validated ciphertext decrypted with the correct view key yields v·G for a real v in [0, 2^64), so a
    // not-found result means the value is outside the file's coverage (a larger file is required) — or the view
    // key does not match the output. There is deliberately no on-the-fly fallback: computing v·G beyond the
    // file's range can take years.
    if results.iter().any(|r| r.is_none()) {
        warn!(
            target: LOG_TARGET,
            "Some balances could not be decrypted: the value is outside the lookup file range {}-{} (a larger \
             lookup file is required), or the provided view key does not match the output(s).",
            lookup.range().start(),
            lookup.range().end(),
        );
    }

    Ok(BalanceRecovery {
        balances: results,
        // The whole file is searched whatever the caller asked for, so its coverage is the answer's.
        searched: range.coverage(&lookup.range()),
    })
}

/// Reports the configured value lookup table's format and coverage for diagnostics. When no file is
/// configured, `configured` is `false`; a configured-but-unreadable file surfaces as an error.
pub(crate) fn value_lookup_info(lookup_file: Option<&Path>) -> anyhow::Result<StealthUtxosGetValueLookupInfoResponse> {
    let Some(path) = lookup_file else {
        return Ok(StealthUtxosGetValueLookupInfoResponse {
            configured: false,
            path: None,
            format: None,
            min: None,
            max: None,
            prefix_len: None,
            value_len: None,
            num_records: None,
        });
    };

    let file =
        fs::File::open(path).map_err(|e| anyhow!("Unable to load value lookup file '{}': {e}", path.display()))?;
    // SAFETY: We assume the file will not be modified while mapped. Although not enforced (e.g. locks, permissions
    // and other platform specific mechanisms), this is a reasonable assumption for most scenarios.
    let lookup = unsafe { SortedPrefixFileLookup::load(&file) }?;
    let header = lookup.header();
    Ok(StealthUtxosGetValueLookupInfoResponse {
        configured: true,
        path: Some(path.display().to_string()),
        format: Some("sorted_prefix_v1".to_string()),
        min: Some(header.min),
        max: Some(header.max),
        prefix_len: Some(header.prefix_len),
        value_len: Some(header.value_len),
        num_records: Some(lookup.len() as u64),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_span(min: Option<u64>, max: Option<u64>) -> u64 {
        let range = ValueRangeRequest::resolve(min, max).scan_range;
        // Saturating so that an unbounded range reports a failing span rather than panicking here.
        range.end().saturating_sub(*range.start()).saturating_add(1)
    }

    /// A caller naming its own maximum must not widen the scan past what one request may spend.
    #[test]
    fn a_caller_cannot_widen_the_scan() {
        assert_eq!(scan_span(None, Some(u64::MAX)), MAX_NO_FILE_SCAN_CANDIDATES + 1);
        assert_eq!(
            scan_span(Some(1_000_000_000), Some(u64::MAX)),
            MAX_NO_FILE_SCAN_CANDIDATES + 1
        );
        assert_eq!(scan_span(None, None), MAX_NO_FILE_SCAN_CANDIDATES + 1);
        // A minimum near the top of the range leaves less than the ceiling to scan, never more.
        assert_eq!(scan_span(Some(u64::MAX - 1), Some(u64::MAX)), 2);
    }

    #[test]
    fn a_range_within_the_ceiling_is_left_alone() {
        let resolved = ValueRangeRequest::resolve(Some(1_000), Some(2_000));
        assert_eq!(resolved.scan_range, 1_000..=2_000);
    }

    /// The lookup file path reports coverage against what the caller *asked* for, so the unclamped
    /// maximum has to survive resolution.
    #[test]
    fn the_requested_maximum_is_reported_unclamped() {
        let resolved = ValueRangeRequest::resolve(None, Some(u64::MAX));
        assert_eq!(resolved.requested_max, Some(u64::MAX));
    }

    /// A `None` is only interpretable next to what was searched, so a request cut short by the
    /// ceiling has to say so.
    #[test]
    fn a_clamped_request_is_reported_as_clamped() {
        let request = ValueRangeRequest::resolve(None, Some(1_000_000_000));
        let searched = request.coverage(&request.scan_range);

        assert!(searched.clamped);
        assert_eq!(searched.max, MAX_NO_FILE_SCAN_CANDIDATES);
    }

    #[test]
    fn a_request_the_search_covers_is_not_reported_as_clamped() {
        // Within the ceiling, the caller got the range it asked for.
        let request = ValueRangeRequest::resolve(Some(1_000), Some(2_000));
        assert!(!request.coverage(&request.scan_range).clamped);

        // Naming no maximum cannot be cut short, whatever the ceiling is.
        let request = ValueRangeRequest::resolve(None, None);
        assert!(!request.coverage(&request.scan_range).clamped);
    }

    /// With a lookup file the whole file is searched, so the coverage reported is the file's -- and a
    /// caller asking past its end is told the answer stops there.
    #[test]
    fn a_lookup_file_reports_its_own_coverage() {
        let request = ValueRangeRequest::resolve(Some(5), Some(u64::MAX));
        let searched = request.coverage(&(0..=1_000));

        assert_eq!((searched.min, searched.max), (0, 1_000));
        assert!(searched.clamped);
    }

    /// A file that starts above the caller's minimum leaves the bottom of the request unsearched, so
    /// the flag has to fire on that end too.
    #[test]
    fn coverage_short_at_the_bottom_is_reported_as_clamped() {
        let request = ValueRangeRequest::resolve(Some(0), Some(2_000));
        let searched = request.coverage(&(1_000..=2_000));

        assert!(searched.clamped);
        assert_eq!((searched.min, searched.max), (1_000, 2_000));
    }

    /// A file covering more than was asked for is not short at either end.
    #[test]
    fn coverage_wider_than_the_request_is_not_clamped() {
        let request = ValueRangeRequest::resolve(Some(1_000), Some(2_000));
        assert!(!request.coverage(&(0..=5_000)).clamped);
    }

    #[test]
    fn a_minimum_above_the_maximum_scans_one_candidate() {
        let resolved = ValueRangeRequest::resolve(Some(500), Some(100));
        assert_eq!(resolved.scan_range, 500..=500);
    }
}
