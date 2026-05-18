//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::any::type_name;

use anyhow::{Context, anyhow};

pub fn encode_to_vec<T: tari_bor::Encode<()>>(value: &T) -> anyhow::Result<Vec<u8>> {
    let bytes = tari_bor::encode(value).with_context(|| anyhow!("Failed to encode {}", type_name::<T>()))?;
    Ok(bytes)
}

pub fn decode_from_slice<T>(bytes: &[u8]) -> anyhow::Result<T>
where T: for<'b> tari_bor::Decode<'b, ()> {
    let value = tari_bor::decode_exact::<T>(bytes).with_context(|| anyhow!("Failed to decode {}", type_name::<T>()))?;
    Ok(value)
}
