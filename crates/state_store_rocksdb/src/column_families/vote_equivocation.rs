//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::{Epoch, NodeHeight};
use tari_ootle_storage::consensus_models::VoteEquivocation;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::{
    codecs::{DefaultCodec, EpochCodec, KeyPrefix, NodeHeightCodec, PublicKeyCodec},
    column_families::cf_names,
    prefixed,
    traits::{Cf, QueryCf},
};

prefixed!(VoteEquivocationPrefix, KeyPrefix::VoteEquivocations);

/// Key = `(epoch, height, signer)`, each component big-endian encoded, so that an epoch is a prefix
/// range and entries within it read in view order.
pub struct VoteEquivocationCf;

impl Cf for VoteEquivocationCf {
    type Key = (Epoch, NodeHeight, RistrettoPublicKeyBytes);
    type KeyCodec = (EpochCodec, NodeHeightCodec, PublicKeyCodec);
    type Prefix = VoteEquivocationPrefix;
    type Value = VoteEquivocation;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        cf_names::CHAIN_METADATA
    }
}

pub struct ByEpochQuery;

impl QueryCf for ByEpochQuery {
    type Cf = VoteEquivocationCf;
    type Key = Epoch;
    type KeyCodec = EpochCodec;
}
