//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_transaction::TransactionSealSignature;

mod address;
mod signed;
mod transaction;

pub use address::*;
pub use signed::*;
pub use transaction::*;

pub type Signature = TransactionSealSignature;
