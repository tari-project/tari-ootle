//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_ootle_storage::time::PrimitiveDateTime;
use tari_transaction::TransactionId;

use crate::{
    codecs::{DefaultCodec, TransactionIdCodec},
    traits::Cf,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalizedTransactionLinkData {
    pub finalized_at: PrimitiveDateTime,
}

pub struct FinalizedTransactionLinkCf;

impl Cf for FinalizedTransactionLinkCf {
    type Key = TransactionId;
    type KeyCodec = TransactionIdCodec;
    type Value = FinalizedTransactionLinkData;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        "transactions_finalized"
    }
}
