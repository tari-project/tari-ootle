//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! The boundary *contract* — plain Rust records that cross the FFI boundary, plus the one
//! structured error envelope and the conversions to/from the internal `tari_*` types.
//!
//! Everything here is generator-agnostic: no `#[uniffi(...)]` / `#[wasm_bindgen]` /
//! cbindgen attributes. Records are plain, derive `Debug + Clone + PartialEq + Serialize +
//! Deserialize`, and serialize bytes as lowercase hex so fixtures are language-neutral.
//!
//! u64/u128-safety is structural: a `u64` is a Rust `u64`; a `u128` is the explicit
//! [`U128`](numeric::U128) `{ hi, lo }` record. Internal `u128`/`Amount` never crosses the
//! boundary.

pub mod address;
pub mod bytes;
pub mod error;
pub mod generic_intent;
pub mod intent;
pub mod network;
pub mod numeric;
pub mod result;
pub mod stealth;

pub use address::{ComponentAddressStr, ResourceAddressStr};
pub use bytes::{
    BuildSeed,
    EncodedTransactionBytes,
    NonceSecretBytes,
    PublicKeyBytes,
    SecretKeyBytes,
    SignatureBytes,
    TransactionIdBytes,
};
pub use error::OotleSdkError;
pub use generic_intent::{ArgValue, BlobSpec, GenericTransactionIntent, InstructionSpec, encode_arg, workspace_arg};
pub use intent::{InputRef, PublicTransferIntent, TransferRecipient};
pub use network::Network;
pub use numeric::{BoundaryAmount, U128};
pub use result::{
    DiffSummary,
    EventSummary,
    FeeReceipt,
    FinalizedResult,
    LogSummary,
    RejectReason,
    SubmitResult,
    TransactionOutcome,
    UpSubstate,
    abort_code,
};
pub use stealth::{
    CommitmentBytes,
    DecryptedOutput,
    EncryptedDataBytes,
    PerOutputEntropy,
    RangeProofBytes,
    StealthEntropy,
    StealthInputSpec,
    StealthMemo,
    StealthOutputSpec,
    StealthPayTo,
    StealthTransferIntent,
    UtxoTagBytes,
};
