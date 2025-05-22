//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_dan_storage::time::PrimitiveDateTime;
use tari_transaction::TransactionId;

use crate::{
    codecs::{DateTimeCodec, TransactionIdCodec},
    traits::Cf,
};

pub struct FinalizedTransactionLinkCf;

impl Cf for FinalizedTransactionLinkCf {
    type Key = TransactionId;
    type KeyCodec = TransactionIdCodec;
    type Value = PrimitiveDateTime;
    type ValueCodec = DateTimeCodec;

    fn name() -> &'static str {
        "finalized_transactions"
    }
}
