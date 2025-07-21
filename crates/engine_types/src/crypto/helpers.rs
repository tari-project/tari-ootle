//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use lazy_static::lazy_static;
use tari_common_types::types::{CommitmentFactory, PrivateKey};
use tari_crypto::{
    commitment::HomomorphicCommitmentFactory,
    ristretto::{
        bulletproofs_plus::BulletproofsPlusService,
        pedersen::PedersenCommitment,
        RistrettoPublicKey,
        RistrettoSchnorr,
        RistrettoSecretKey,
    },
    tari_utilities::ByteArray,
};
use tari_template_lib::{prelude::BalanceProofSignature, types::Amount};

use crate::FromByteType;

lazy_static! {
    /// Static reference to the default commitment factory. Each instance of CommitmentFactory requires a number of heap allocations.
    static ref COMMITMENT_FACTORY: CommitmentFactory = CommitmentFactory::default();
    /// Static reference to the default range proof service. Each instance of RangeProofService requires a number of heap allocations.
    static ref RANGE_PROOF_AGG_1_SERVICE: BulletproofsPlusService =
        BulletproofsPlusService::init(64, 1, CommitmentFactory::default()).unwrap();
    static ref RANGE_PROOF_AGG_2_SERVICE: BulletproofsPlusService =
        BulletproofsPlusService::init(64, 2, CommitmentFactory::default()).unwrap();
}

pub fn get_range_proof_service(aggregation_factor: usize) -> &'static BulletproofsPlusService {
    match aggregation_factor {
        1 => &RANGE_PROOF_AGG_1_SERVICE,
        2 => &RANGE_PROOF_AGG_2_SERVICE,
        _ => panic!(
            "Unsupported BP aggregation factor {}. Expected 1 or 2",
            aggregation_factor
        ),
    }
}

pub fn get_commitment_factory() -> &'static CommitmentFactory {
    &COMMITMENT_FACTORY
}

/// Creates a Pedersen commitment to the given amount using the provided mask.
///
/// # Panics
/// Panics if the amount is not positive.
pub fn commit_amount(mask: &RistrettoSecretKey, amount: Amount) -> PedersenCommitment {
    commit_amount_checked(mask, amount).expect("commitment amount is negative")
}

/// Creates a Pedersen commitment to the given amount using the provided mask.
///
/// # Returns
/// Returns `None` if the amount is negative, otherwise returns a `PedersenCommitment`.
pub fn commit_amount_checked(mask: &RistrettoSecretKey, amount: Amount) -> Option<PedersenCommitment> {
    let v = convert_amount_to_secret(&amount)?;
    Some(get_commitment_factory().commit(mask, &v))
}

/// Converts a `Amount` to a `RistrettoSecretKey`.
/// # Returns
/// Returns `None` if the amount is negative, otherwise returns a `RistrettoSecretKey`.
pub fn convert_amount_to_secret(amount: &Amount) -> Option<RistrettoSecretKey> {
    if amount.is_negative() {
        return None;
    }

    let mut val_bytes = [0u8; 32];
    val_bytes[..Amount::BYTE_SIZE].copy_from_slice(&amount.to_le_bytes());
    Some(
        RistrettoSecretKey::from_canonical_bytes(&val_bytes)
            .expect("MSB in 256 bit integer is always zero and < ell (Ristretto base point) therefore canonical"),
    )
}

pub fn try_decode_to_signature(balance_proof: &BalanceProofSignature) -> Option<RistrettoSchnorr> {
    let public_nonce = RistrettoPublicKey::try_from_byte_type(balance_proof.public_nonce()).ok()?;
    let signature = PrivateKey::from_canonical_bytes(balance_proof.signature().as_bytes()).ok()?;
    Some(RistrettoSchnorr::new(public_nonce, signature))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn homomorphic() {
        let amount1 = Amount::MAX;
        let commitment1 = commit_amount_checked(&Default::default(), amount1).unwrap();
        let amount2 = Amount::from(1_000);
        let commitment2 = commit_amount_checked(&Default::default(), amount2).unwrap();

        let resulting_commitment = commitment1.as_public_key() - commitment2.as_public_key();

        let check = commit_amount_checked(&Default::default(), amount1 - amount2).unwrap();
        assert_eq!(resulting_commitment, *check.as_public_key());
    }

    #[test]
    fn negative() {
        let amount = Amount::from(-1);
        assert!(commit_amount_checked(&Default::default(), amount).is_none());
        let amount = Amount::MIN;
        assert!(commit_amount_checked(&Default::default(), amount).is_none());

        let amount = -Amount::from(199999999999999999u128);
        assert!(convert_amount_to_secret(&amount).is_none());
    }

    #[test]
    fn endianness() {
        // Check that the endianness used in RistrettoSecretKey (dalek Scalar) is the same as
        // convert_big_amount_to_secret.
        let amount = Amount::from(199999999999999999u128);
        let v1 = convert_amount_to_secret(&amount).unwrap();

        assert_eq!(v1.as_bytes()[..Amount::BYTE_SIZE], amount.to_le_bytes());

        let v2 = RistrettoSecretKey::from(1000);
        let sub = Amount::from_le_slice((v1 - v2).as_bytes()).unwrap();
        let expected = amount - Amount::from(1000);
        assert_eq!(sub, expected);
    }
}
