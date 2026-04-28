//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::Epoch;
use tari_ootle_storage::consensus_models::RollbackHistoryEntry;

use crate::{
    codecs::{DefaultCodec, EpochCodec, KeyPrefix, NumberCodec},
    column_families::cf_names,
    prefixed,
    traits::Cf,
};

prefixed!(RollbackHistoryPrefix, KeyPrefix::RollbackHistory);

/// Key = `(applied_at_unix_secs, target_epoch)` — both big-endian encoded so entries
/// sort chronologically, with `target_epoch` as a tiebreaker when two rollbacks happen
/// to be recorded in the same second (rare, but possible in tests).
pub struct RollbackHistoryCf;

impl Cf for RollbackHistoryCf {
    type Key = (u64, Epoch);
    type KeyCodec = (NumberCodec<u64>, EpochCodec);
    type Prefix = RollbackHistoryPrefix;
    type Value = RollbackHistoryEntry;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        cf_names::CHAIN_METADATA
    }
}
