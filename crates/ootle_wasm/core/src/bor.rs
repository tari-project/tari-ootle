//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use base64::{Engine as _, engine::general_purpose::STANDARD};
use tari_ootle_transaction::Transaction;

use crate::error::OotleWasmError;

/// BOR-encode a Transaction and return the result as a base64 string (TransactionEnvelope format).
pub fn bor_encode_transaction(transaction: &Transaction) -> Result<String, OotleWasmError> {
    let bytes = tari_bor::encode(transaction)?;
    Ok(STANDARD.encode(&bytes))
}

/// BOR-encode a Transaction from a JSON string and return the result as a base64 string.
pub fn bor_encode_transaction_json(transaction_json: &str) -> Result<String, OotleWasmError> {
    let tx: Transaction = serde_json::from_str(transaction_json)?;
    bor_encode_transaction(&tx)
}
