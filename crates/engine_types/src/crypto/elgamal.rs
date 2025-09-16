//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use borsh::BorshSerialize;
use tari_bor::{Deserialize, Serialize};
use tari_common_types::types::PrivateKey;
use tari_crypto::{
    commitment::HomomorphicCommitmentFactory,
    keys::{PublicKey, SecretKey},
    ristretto::{pedersen::PedersenCommitment, RistrettoPublicKey, RistrettoSecretKey},
    tari_utilities,
    tari_utilities::ByteArray,
};
use tari_template_lib::{models::ViewableBalanceProof, types::crypto::RistrettoPublicKeyBytes};

use crate::{
    crypto::{get_commitment_factory, messages, value_lookup_table::ValueLookupTable},
    resource_container::ResourceError,
    ConvertFromByteType,
    ToByteType,
};

pub fn validate_elgamal_verifiable_balance_proof(
    commitment: &PedersenCommitment,
    view_key: Option<&RistrettoPublicKey>,
    viewable_balance_proof: Option<&ViewableBalanceProof>,
) -> Result<Option<ElgamalVerifiableBalance>, ResourceError> {
    // Check that if a view key is provided, then a viewable balance proof is also provided and vice versa
    let Some(view_key) = view_key else {
        if viewable_balance_proof.is_none() {
            return Ok(None);
        }
        return Err(ResourceError::InvalidConfidentialProof {
            details: "ViewableBalanceProof provided for a resource that is not viewable".to_string(),
        });
    };

    let Some(proof) = viewable_balance_proof else {
        return Err(ResourceError::InvalidConfidentialProof {
            details: "ViewableBalanceProof is required for a viewable resource".to_string(),
        });
    };

    // Decode and check that each field is well-formed
    let encrypted = RistrettoPublicKey::from_canonical_bytes(&*proof.elgamal_encrypted).map_err(|_| {
        ResourceError::InvalidConfidentialProof {
            details: "Invalid value for E".to_string(),
        }
    })?;

    let elgamal_public_nonce =
        RistrettoPublicKey::from_canonical_bytes(&*proof.elgamal_public_nonce).map_err(|_| {
            ResourceError::InvalidConfidentialProof {
                details: "Invalid public key for R".to_string(),
            }
        })?;

    let c_prime = PedersenCommitment::from_canonical_bytes(&*proof.c_prime).map_err(|_| {
        ResourceError::InvalidConfidentialProof {
            details: "Invalid commitment for C'".to_string(),
        }
    })?;

    let e_prime = PedersenCommitment::from_canonical_bytes(&*proof.e_prime).map_err(|_| {
        ResourceError::InvalidConfidentialProof {
            details: "Invalid commitment for E'".to_string(),
        }
    })?;

    let r_prime = RistrettoPublicKey::from_canonical_bytes(&*proof.r_prime).map_err(|_| {
        ResourceError::InvalidConfidentialProof {
            details: "Invalid public key for R'".to_string(),
        }
    })?;

    let s_v = PrivateKey::from_canonical_bytes(&*proof.s_v).map_err(|_| ResourceError::InvalidConfidentialProof {
        details: "Invalid private key for s_v".to_string(),
    })?;

    let s_m = PrivateKey::from_canonical_bytes(&*proof.s_m).map_err(|_| ResourceError::InvalidConfidentialProof {
        details: "Invalid private key for s_m".to_string(),
    })?;

    let s_r = &PrivateKey::from_canonical_bytes(&*proof.s_r).map_err(|_| ResourceError::InvalidConfidentialProof {
        details: "Invalid private key for s_r".to_string(),
    })?;

    // Fiat-Shamir challenge
    let e = &RistrettoSecretKey::from_uniform_bytes(&messages::viewable_balance_proof64(
        commitment,
        view_key,
        proof.as_challenge_fields(),
    ))
        // TODO: it would be better if from_uniform_bytes took a [u8; 64]
        .expect("INVARIANT VIOLATION: RistrettoSecretKey::from_uniform_bytes and hash output length mismatch");

    // Check eC + C' ?= s_m.G + sv.H
    let left = e * commitment.as_public_key() + c_prime.as_public_key();
    let right = get_commitment_factory().commit(&s_m, &s_v);
    if left != *right.as_public_key() {
        return Err(ResourceError::InvalidConfidentialProof {
            details: "Invalid viewable balance proof (eC + C' != s_m.G + s_v.H)".to_string(),
        });
    }

    // Check eE + E' ?= s_v.G + s_r.P
    let left = e * &encrypted + e_prime.as_public_key();
    let right = RistrettoPublicKey::from_secret_key(&s_v) + s_r * view_key;
    if left != right {
        return Err(ResourceError::InvalidConfidentialProof {
            details: "Invalid viewable balance proof (eE + E' != s_v.G + s_r.P)".to_string(),
        });
    }

    // Check eR + R' ?= s_r.G
    let left = e * &elgamal_public_nonce + r_prime;
    let right = RistrettoPublicKey::from_secret_key(s_r);
    if left != right {
        return Err(ResourceError::InvalidConfidentialProof {
            details: "Invalid viewable balance proof (eR + R' != s_r.G)".to_string(),
        });
    }

    Ok(Some(ElgamalVerifiableBalance {
        encrypted,
        public_nonce: elgamal_public_nonce,
    }))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CompressedElgamalVerifiableBalance {
    pub encrypted: RistrettoPublicKeyBytes,
    pub public_nonce: RistrettoPublicKeyBytes,
}

impl ConvertFromByteType<CompressedElgamalVerifiableBalance> for ElgamalVerifiableBalance {
    type Error = tari_utilities::ByteArrayError;

    fn convert_from_byte_type(bytes: &CompressedElgamalVerifiableBalance) -> Result<Self, Self::Error> {
        let encrypted = RistrettoPublicKey::convert_from_byte_type(&bytes.encrypted)?;
        let public_nonce = RistrettoPublicKey::convert_from_byte_type(&bytes.public_nonce)?;
        Ok(ElgamalVerifiableBalance {
            encrypted,
            public_nonce,
        })
    }
}

impl From<ElgamalVerifiableBalance> for CompressedElgamalVerifiableBalance {
    fn from(value: ElgamalVerifiableBalance) -> Self {
        (&value).into()
    }
}

impl From<&ElgamalVerifiableBalance> for CompressedElgamalVerifiableBalance {
    fn from(value: &ElgamalVerifiableBalance) -> Self {
        Self {
            encrypted: value.encrypted.to_byte_type(),
            public_nonce: value.public_nonce.to_byte_type(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElgamalVerifiableBalance {
    pub encrypted: RistrettoPublicKey,
    pub public_nonce: RistrettoPublicKey,
}

impl ElgamalVerifiableBalance {
    pub fn brute_force_balance<I: IntoIterator<Item = u64>, TLookup: ValueLookupTable>(
        &self,
        view_private_key: &RistrettoSecretKey,
        value_range: I,
        lookup_table: &mut TLookup,
    ) -> Result<Option<u64>, TLookup::Error> {
        let mut result = Self::batched_brute_force(view_private_key, value_range, lookup_table, Some(self))?;
        Ok(result.pop().flatten())
    }

    pub fn batched_brute_force<'a, IValueRange, TLookup, IBalances>(
        view_private_key: &RistrettoSecretKey,
        value_range: IValueRange,
        lookup_table: &mut TLookup,
        verifiable_balances: IBalances,
    ) -> Result<Vec<Option<u64>>, TLookup::Error>
    where
        IValueRange: IntoIterator<Item = u64>,
        TLookup: ValueLookupTable,
        IBalances: IntoIterator<Item = &'a Self>,
    {
        let mut balances = verifiable_balances
            .into_iter()
            .enumerate()
            .map(|(i, balance)| {
                // V = E - pR
                let balance = &balance.encrypted - view_private_key * &balance.public_nonce;
                (i, balance.to_byte_type())
            })
            .collect::<Vec<_>>();

        let mut results = vec![None; balances.len()];

        for v in value_range {
            let value = lookup_table.lookup(v)?.unwrap_or_else(|| {
                // Fallback to slow lookup method if the lookup table does not contain a key for the value
                let pk = RistrettoPublicKey::from_secret_key(&RistrettoSecretKey::from(v));
                copy_fixed(pk.as_bytes())
            });

            while let Some(pos) = balances.iter().position(|(_, balance)| value == balance.as_bytes()) {
                let (order, _) = balances.swap_remove(pos);
                results
                    .get_mut(order)
                    .expect("batched_brute_force: balances index greater than results")
                    .replace(v);
            }

            if balances.is_empty() {
                break;
            }
        }

        Ok(results)
    }
}

impl TryFrom<&CompressedElgamalVerifiableBalance> for ElgamalVerifiableBalance {
    type Error = tari_utilities::ByteArrayError;

    fn try_from(value: &CompressedElgamalVerifiableBalance) -> Result<Self, Self::Error> {
        let encrypted = RistrettoPublicKey::convert_from_byte_type(&value.encrypted)?;
        let public_nonce = RistrettoPublicKey::convert_from_byte_type(&value.public_nonce)?;
        Ok(ElgamalVerifiableBalance {
            encrypted,
            public_nonce,
        })
    }
}

impl ToByteType for ElgamalVerifiableBalance {
    type ByteType = CompressedElgamalVerifiableBalance;

    fn to_byte_type(&self) -> Self::ByteType {
        CompressedElgamalVerifiableBalance {
            encrypted: self.encrypted.to_byte_type(),
            public_nonce: self.public_nonce.to_byte_type(),
        }
    }
}

fn copy_fixed(src: &[u8]) -> [u8; 32] {
    let mut buf = [0u8; 32];
    buf.copy_from_slice(src);
    buf
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use rand::rngs::OsRng;
    use tari_crypto::keys::SecretKey;

    use super::*;

    #[derive(Default)]
    pub struct TestLookupTable;

    impl ValueLookupTable for TestLookupTable {
        type Error = Infallible;

        fn lookup(&mut self, value: u64) -> Result<Option<[u8; 32]>, Self::Error> {
            // This would be a sequential lookup in a real implementation
            Ok(Some(copy_fixed(
                RistrettoPublicKey::from_secret_key(&RistrettoSecretKey::from(value)).as_bytes(),
            )))
        }
    }

    mod brute_force_balance {
        use tari_crypto::{
            keys::PublicKey,
            ristretto::{RistrettoPublicKey, RistrettoSecretKey},
        };

        use super::*;

        #[test]
        fn it_finds_the_value() {
            const VALUE: u64 = 5242;
            let view_sk = &RistrettoSecretKey::random(&mut OsRng);
            let (nonce_sk, nonce_pk) = RistrettoPublicKey::random_keypair(&mut OsRng);

            let rp = nonce_sk * view_sk;

            let subject = ElgamalVerifiableBalance {
                encrypted: RistrettoPublicKey::from_secret_key(&rp) +
                    RistrettoPublicKey::from_secret_key(&RistrettoSecretKey::from(VALUE)),
                public_nonce: nonce_pk,
            };

            let balance = subject
                .brute_force_balance(view_sk, 0..=10000, &mut TestLookupTable)
                .unwrap();
            assert_eq!(balance, Some(VALUE));
        }

        #[test]
        fn it_returns_the_value_equal_to_max_value() {
            let view_sk = &RistrettoSecretKey::random(&mut OsRng);
            let (nonce_sk, nonce_pk) = RistrettoPublicKey::random_keypair(&mut OsRng);

            let rp = nonce_sk * view_sk;

            let subject = ElgamalVerifiableBalance {
                encrypted: RistrettoPublicKey::from_secret_key(&rp) +
                    RistrettoPublicKey::from_secret_key(&RistrettoSecretKey::from(10)),
                public_nonce: nonce_pk,
            };

            let balance = subject
                .brute_force_balance(view_sk, 0..=10, &mut TestLookupTable)
                .unwrap();
            assert_eq!(balance, Some(10));

            let balance = subject
                .brute_force_balance(view_sk, 10..=12, &mut TestLookupTable)
                .unwrap();
            assert_eq!(balance, Some(10));
        }

        #[test]
        fn it_returns_none_if_the_value_out_of_range() {
            let subject = ElgamalVerifiableBalance {
                encrypted: RistrettoPublicKey::from_secret_key(&RistrettoSecretKey::from(101)),
                public_nonce: Default::default(),
            };

            let balance = subject
                .brute_force_balance(&RistrettoSecretKey::default(), 0..=100, &mut TestLookupTable)
                .unwrap();
            assert_eq!(balance, None);

            let balance = subject
                .brute_force_balance(&RistrettoSecretKey::default(), 102..=103, &mut TestLookupTable)
                .unwrap();
            assert_eq!(balance, None);
        }

        #[test]
        fn it_brute_forces_a_batch() {
            let view_sk = &RistrettoSecretKey::random(&mut OsRng);

            let subject = (0..100)
                .map(|v| {
                    let (nonce_sk, nonce_pk) = RistrettoPublicKey::random_keypair(&mut OsRng);
                    let rp = nonce_sk * view_sk;
                    ElgamalVerifiableBalance {
                        encrypted: (RistrettoPublicKey::from_secret_key(&rp) +
                            RistrettoPublicKey::from_secret_key(&RistrettoSecretKey::from(v))),
                        public_nonce: nonce_pk,
                    }
                })
                .collect::<Vec<_>>();

            let balances =
                ElgamalVerifiableBalance::batched_brute_force(view_sk, 0..=10000, &mut TestLookupTable, subject.iter())
                    .unwrap();
            assert_eq!(balances.len(), 100);
            for (i, balance) in balances.into_iter().enumerate() {
                assert_eq!(balance, Some(i as u64));
            }
        }
    }
}
