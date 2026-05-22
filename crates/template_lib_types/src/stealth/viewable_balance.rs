//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};

use crate::crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes, Scalar32Bytes};

/// ### Verifiable encryption
///
/// A verifiable ElGamal encryption proving system that asserts the value bound to a Pedersen
/// commitment matches the value encrypted to a given public key. This will be used to assert that the issuer can
/// decrypt account balances without knowing the opening to the account's balance commitment.
///
/// The proving relation is $\\{ (C, E, R, P); (v, m, r) | C = mG + vH, E = vG + rP, R = rG \\}$.
///
/// The prover samples $x_v, x_m, x_r$ uniformly at random.
/// It computes $C' = x_v H + x_m G$, $E' = x_v G + x_r P$, and $R' = x_r G$ and sends them to the verifier.
/// The verifier samples nonzero $e$ uniformly at random and sends it to the prover.
/// The prover computes $s_v = ev + x_v$, $s_m = em + x_m$, and $s_r = er + x_r$ and sends them to the verifier.
/// The verifier accepts the proof if and only if $eC + C' = s_v H + s_m G$, $eE + E' = s_v G + s_r P$, and $eR + R' =
/// s_r G$.
///
/// It is a sigma protocol for the relation that is complete, $2$-special sound, and special honest-verifier zero
/// knowledge.
///
/// The proof size is static (256 bytes).
#[derive(Debug, Clone, Encode, Decode, CborLen, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct ViewableBalanceProof {
    /// The encrypted value that takes the form: E = v.G + r.P
    /// where v is the value, G is the generator, r is the secret_nonce and P is the view key.
    /// The value is decrypted by brute forcing E - R.p = v.G
    #[n(0)]
    pub elgamal_encrypted: RistrettoPublicKeyBytes,
    /// The public nonce used in the ElGamal encryption R = r.G
    #[n(1)]
    pub elgamal_public_nonce: RistrettoPublicKeyBytes,
    /// Part of the proof that the encrypted value is correctly constructed. C' = x_v.H + x_m.G
    #[n(2)]
    pub c_prime: PedersenCommitmentBytes,
    /// Part of the proof that the encrypted value is correctly constructed. E' = x_v.G + x_r.P
    #[n(3)]
    pub e_prime: RistrettoPublicKeyBytes,
    /// Part of the proof that the encrypted value is correctly constructed. R' = x_r.G
    #[n(4)]
    pub r_prime: RistrettoPublicKeyBytes,
    /// Part of the proof that the encrypted value is correctly constructed. s_v = x_v + e.v
    #[n(5)]
    pub s_v: Scalar32Bytes,
    /// Part of the proof that the encrypted value is correctly constructed. s_m = x_m + e.m
    #[n(6)]
    pub s_m: Scalar32Bytes,
    /// Part of the proof that the encrypted value is correctly constructed. s_r = x_r + e.r
    #[n(7)]
    pub s_r: Scalar32Bytes,
}

impl ViewableBalanceProof {
    pub fn as_challenge_fields(&self) -> ViewableBalanceProofMessageFields<'_> {
        ViewableBalanceProofMessageFields {
            elgamal_encrypted: &self.elgamal_encrypted,
            elgamal_public_nonce: &self.elgamal_public_nonce,
            c_prime: &self.c_prime,
            e_prime: &self.e_prime,
            r_prime: &self.r_prime,
        }
    }
}

#[derive(Clone, Copy)]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct ViewableBalanceProofMessageFields<'a> {
    pub elgamal_encrypted: &'a RistrettoPublicKeyBytes,
    pub elgamal_public_nonce: &'a RistrettoPublicKeyBytes,
    pub c_prime: &'a PedersenCommitmentBytes,
    pub e_prime: &'a RistrettoPublicKeyBytes,
    pub r_prime: &'a RistrettoPublicKeyBytes,
}
