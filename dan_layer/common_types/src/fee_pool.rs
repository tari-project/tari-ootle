//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::ValidatorFeePoolAddress;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::{shard::Shard, uint::U256, NumPreshards};

pub fn derive_fee_pool_address(
    public_key_bytes: &RistrettoPublicKeyBytes,
    num_preshards: NumPreshards,
    shard: Shard,
) -> ValidatorFeePoolAddress {
    let mut masked_public_key_bytes = [0u8; 32];
    // We offset the start shard by 128 LSBs of the public key's MSB (big-endian)
    masked_public_key_bytes[16..].copy_from_slice(&public_key_bytes.as_slice()[..16]);
    let range = shard.to_substate_address_range(num_preshards);
    let offset_addr = range.start().to_u256() + U256::from_be_bytes(masked_public_key_bytes);
    ValidatorFeePoolAddress::from_array(offset_addr.to_be_bytes())
}

#[cfg(test)]
mod tests {
    use rand::rngs::OsRng;
    use tari_crypto::{keys::PublicKey, ristretto::RistrettoPublicKey};
    use tari_engine_types::ToByteType;

    use super::*;
    use crate::SubstateAddress;

    #[test]
    fn it_creates_a_pool_address_that_naturally_falls_in_the_shard() {
        let pk = RistrettoPublicKeyBytes::from([0xff; 32]);
        let num_preshards = NumPreshards::P256;
        let fee_pool_address = derive_fee_pool_address(&pk, num_preshards, Shard::from(1));
        let addr = SubstateAddress::from_substate_id(&fee_pool_address.into(), 0);
        let shard = addr.to_shard(num_preshards);

        assert_eq!(shard, Shard::from(1));

        let (_, pk) = RistrettoPublicKey::random_keypair(&mut OsRng);
        let fee_pool_address = derive_fee_pool_address(&pk.to_byte_type(), num_preshards, Shard::from(212));
        let addr = SubstateAddress::from_substate_id(&fee_pool_address.into(), 0);
        let shard = addr.to_shard(num_preshards);
        assert_eq!(shard, Shard::from(212));
    }
}
