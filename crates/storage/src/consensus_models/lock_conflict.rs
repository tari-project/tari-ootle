//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};
use serde::{Deserialize, Serialize};
use tari_ootle_common_types::SubstateLockType;
use tari_ootle_transaction::TransactionId;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Encode, Decode, CborLen)]
pub struct LockConflict {
    #[n(0)]
    pub transaction_id: TransactionId,
    #[n(1)]
    pub existing_lock: SubstateLockType,
    #[n(2)]
    pub requested_lock: SubstateLockType,
    #[n(3)]
    pub is_local_only: bool,
}
