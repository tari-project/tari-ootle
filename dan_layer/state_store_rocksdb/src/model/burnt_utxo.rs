//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_dan_storage::consensus_models::BlockId;
use tari_engine_types::{
    confidential::UnclaimedConfidentialOutput,
    template_models::UnclaimedConfidentialOutputAddress,
};

use crate::{
    codecs::{BlockIdCodec, BytesCodec, DefaultCodec, TupleBytesCodec, UnitCodec},
    traits::{Cf, QueryCf},
};

pub struct BurntUtxoModel;

impl Cf for BurntUtxoModel {
    type Key = UnclaimedConfidentialOutputAddress;
    type KeyCodec = BytesCodec;
    type Value = UnclaimedConfidentialOutput;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        "burnt_utxos"
    }
}

pub struct ProposedInBlockIndex;

impl Cf for ProposedInBlockIndex {
    type Key = (BlockId, UnclaimedConfidentialOutputAddress);
    type KeyCodec = TupleBytesCodec<Self::Key>;
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
