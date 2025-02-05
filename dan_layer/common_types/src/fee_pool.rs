//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::vn_fee_pool::ValidatorFeePoolAddress;

use crate::{shard::Shard, uint::U256, NumPreshards};

pub fn derive_fee_pool_address(
    public_key_bytes: [u8; 32],
    num_preshards: NumPreshards,
    shard: Shard,
) -> ValidatorFeePoolAddress {
    let mut masked_public_key_bytes = [0u8; 32];
    // We offset the start shard by 128 LSBs of the public key's MSB (big-endian)
    masked_public_key_bytes[16..].copy_from_slice(&public_key_bytes[..16]);
    let range = shard.to_substate_address_range(num_preshards);
    let offset_addr = range.start().to_u256() + U256::from_be_bytes(masked_public_key_bytes);
    ValidatorFeePoolAddress::from_array(offset_addr.to_be_bytes())
}
