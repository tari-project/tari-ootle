//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_consensus_types::BlockId;
use tari_engine_types::{
    confidential::UnclaimedConfidentialOutput,
    template_lib_models::UnclaimedConfidentialOutputAddress,
};
use tari_template_lib_types::ObjectKey;

use crate::{
    codecs::{BlockIdCodec, BytesCodec, DefaultVersionedCodec, FixedBytesCodec, UnitCodec},
    traits::{Cf, QueryCf, VersionedUnclaimedConfidentialOutput},
};

pub struct BurntUtxoCf;

impl Cf for BurntUtxoCf {
    type Key = UnclaimedConfidentialOutputAddress;
    type KeyCodec = BytesCodec;
    type Value = UnclaimedConfidentialOutput;
    type ValueCodec = DefaultVersionedCodec<VersionedUnclaimedConfidentialOutput>;

    fn name() -> &'static str {
        "burnt_utxos"
    }
}

pub struct ProposedInBlockIndex;

impl Cf for ProposedInBlockIndex {
    type Key = (BlockId, UnclaimedConfidentialOutputAddress);
    type KeyCodec = (BlockIdCodec, FixedBytesCodec<{ ObjectKey::LENGTH }>);
    type Value = ();
    type ValueCodec = UnitCodec;

    fn name() -> &'static str {
        "burnt_utxos_proposedin_idx"
    }
}

pub struct ByProposedInBlockIdQuery;

impl QueryCf for ByProposedInBlockIdQuery {
    type Cf = ProposedInBlockIndex;
    type Key = BlockId;
    type KeyCodec = BlockIdCodec;
}
