//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};
use serde::{Deserialize, Serialize};
use tari_ootle_storage::time::PrimitiveDateTime;
use tari_ootle_transaction::TransactionId;

use crate::{
    codecs::{DefaultCodec, KeyPrefix, TransactionIdCodec},
    column_families::cf_names,
    prefixed,
    traits::Cf,
};

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, CborLen)]
pub struct FinalizedTransactionLinkData {
    // PrimitiveDateTime (from `time` crate) only implements serde, so bridge it.
    #[n(0)]
    #[cbor(with = "tari_bor::adapters::serde_bridge")]
    pub finalized_at: PrimitiveDateTime,
}

prefixed!(FinalizedTransactionLinkPrefix, KeyPrefix::FinalizedTransactionLinks);

pub struct FinalizedTransactionLinkCf;

impl Cf for FinalizedTransactionLinkCf {
    type Key = TransactionId;
    type KeyCodec = TransactionIdCodec;
    type Prefix = FinalizedTransactionLinkPrefix;
    type Value = FinalizedTransactionLinkData;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        cf_names::TRANSACTIONS
    }
}
