//  Copyright 2022 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

//! Utilities to get random values inside templates

use tari_template_abi::{EngineOp, call_engine, rust::prelude::*};

use crate::args::{GenerateRandomAction, GenerateRandomInvokeArg, InvokeResult};

/// Returns `len` pseudorandom bytes.
/// WARNING: Entropy is provided by the transaction itself to ensure deterministic execution. This is not suitable for
/// cryptographic purposes.
/// NOTE: you cannot use the `rand` crate in templates, because `get_random` is not available for
/// `wasm32-unknown-unknown` target.
pub fn random_bytes(len: u32) -> Vec<u8> {
    let resp: InvokeResult = call_engine(EngineOp::GenerateRandomInvoke, &GenerateRandomInvokeArg {
        action: GenerateRandomAction::GetRandomBytes { len },
    });
    resp.decode().expect("Failed to decode random bytes")
}

/// Returns a `u32` representing a random value
pub fn random_u32() -> u32 {
    let v = random_bytes(4);
    u32::from_le_bytes(v.as_slice().try_into().unwrap())
}
