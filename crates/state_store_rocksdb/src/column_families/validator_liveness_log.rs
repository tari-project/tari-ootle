//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::{Epoch, NodeHeight};
use tari_ootle_storage::consensus_models::LivenessCounters;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::{
    codecs::{DefaultCodec, EpochCodec, KeyPrefix, NodeHeightCodec, PublicKeyCodec},
    column_families::cf_names,
    prefixed,
    traits::{Cf, QueryCf},
};

prefixed!(ValidatorLivenessLogPrefix, KeyPrefix::ValidatorLivenessLog);

/// Key = `(epoch, validator, committed_height)`, each component big-endian encoded, so that one
/// validator's history within an epoch is a prefix range that reads in height order. The state as of
/// a height is then the last entry at or below it.
///
/// An entry is appended only when a commit moves one of the counters, which is a few entries per
/// epoch for a stable validator.
pub struct ValidatorLivenessLogCf;

impl Cf for ValidatorLivenessLogCf {
    type Key = (Epoch, RistrettoPublicKeyBytes, NodeHeight);
    type KeyCodec = (EpochCodec, PublicKeyCodec, NodeHeightCodec);
    type Prefix = ValidatorLivenessLogPrefix;
    type Value = LivenessCounters;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        cf_names::CHAIN_METADATA
    }
}

pub struct ByEpochQuery;

impl QueryCf for ByEpochQuery {
    type Cf = ValidatorLivenessLogCf;
    type Key = Epoch;
    type KeyCodec = EpochCodec;
}

pub struct ByValidatorQuery;

impl QueryCf for ByValidatorQuery {
    type Cf = ValidatorLivenessLogCf;
    type Key = (Epoch, RistrettoPublicKeyBytes);
    type KeyCodec = (EpochCodec, PublicKeyCodec);
}
