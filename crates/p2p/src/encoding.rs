//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::any::type_name;

use anyhow::{Context, anyhow};
use tari_engine_types::limits::MAX_CBOR_NESTING_DEPTH;

pub fn encode_to_vec<T: tari_bor::Encode<()>>(value: &T) -> anyhow::Result<Vec<u8>> {
    let bytes = tari_bor::encode(value).with_context(|| anyhow!("Failed to encode {}", type_name::<T>()))?;
    Ok(bytes)
}

pub fn decode_from_slice<T>(bytes: &[u8]) -> anyhow::Result<T>
where T: for<'b> tari_bor::Decode<'b, ()> {
    // Bytes off the wire, so the nesting bound applies before the target type's own decode
    // recurses through them.
    let value = tari_bor::decode_exact_with_max_depth::<T>(bytes, MAX_CBOR_NESTING_DEPTH)
        .with_context(|| anyhow!("Failed to decode {}", type_name::<T>()))?;
    Ok(value)
}
