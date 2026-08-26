//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Groth16/BN254 verification implemented entirely in WASM, to measure what a pure-template zk
//! verifier costs against the per-transaction metering budget
//! (`limits::MAX_WASM_POINTS_PER_TRANSACTION`).
//!
//! The functions split the verify into its separately-priced parts so the driver
//! (`crates/engine/examples/zk_points_calibrate.rs`) can attribute points:
//!
//! - `noop` — fixed per-call overhead to subtract from every other measurement.
//! - `decode_*` — deserialization only, with and without subgroup checks.
//! - `prepare` — `VerifyingKey` -> `PreparedVerifyingKey` (one `alpha_g1_beta_g2` pairing).
//! - `verify_*` — the full check, from each of the three input encodings a caller might store.
//!
//! `_unchecked` variants skip the subgroup checks on deserialized points. They exist to price
//! those checks, not to offer a faster path: skipping them on caller-supplied data is unsound.

use ark_bn254::{Bn254, Fr};
use ark_groth16::{Groth16, PreparedVerifyingKey, Proof, VerifyingKey, prepare_verifying_key};
use ark_serialize::CanonicalDeserialize;
use ark_snark::SNARK;
use tari_template_lib::prelude::*;

fn decode_inputs(bytes: &[u8]) -> Vec<Fr> {
    Vec::<Fr>::deserialize_uncompressed(bytes).expect("bad public inputs")
}

#[template]
mod zk_verifier {
    use super::*;

    pub struct ZkVerifier {
        /// Uncompressed `PreparedVerifyingKey`. Held in component state so the stateful path
        /// measures what a real contract pays: the caller supplies only proof and inputs.
        pvk: Bytes,
    }

    impl ZkVerifier {
        pub fn new(pvk: Bytes) -> Component<Self> {
            Component::new(Self { pvk })
                .with_access_rules(AccessRules::allow_all())
                .create()
        }

        /// Fixed cost of entering a template call with a byte argument of the same shape.
        pub fn noop(bytes: Bytes) -> bool {
            !bytes.is_empty()
        }

        // --- deserialization only ---------------------------------------------------------

        pub fn decode_proof_compressed(proof: Bytes) -> bool {
            Proof::<Bn254>::deserialize_compressed(proof.as_slice()).is_ok()
        }

        pub fn decode_proof_uncompressed(proof: Bytes) -> bool {
            Proof::<Bn254>::deserialize_uncompressed(proof.as_slice()).is_ok()
        }

        pub fn decode_proof_uncompressed_unchecked(proof: Bytes) -> bool {
            Proof::<Bn254>::deserialize_uncompressed_unchecked(proof.as_slice()).is_ok()
        }

        pub fn decode_vk_uncompressed(vk: Bytes) -> bool {
            VerifyingKey::<Bn254>::deserialize_uncompressed(vk.as_slice()).is_ok()
        }

        // --- verifying key preparation ----------------------------------------------------

        /// Decode a `VerifyingKey` and prepare it. The result is what `new` stores, so a contract
        /// pays this once at deploy rather than per verify.
        pub fn prepare(vk: Bytes) -> bool {
            let vk = VerifyingKey::<Bn254>::deserialize_uncompressed(vk.as_slice()).expect("bad vk");
            let pvk = prepare_verifying_key(&vk);
            !pvk.vk.gamma_abc_g1.is_empty()
        }

        // --- full verification ------------------------------------------------------------

        /// Worst case: unprepared verifying key supplied per call, compressed encodings
        /// throughout. Compression halves the bytes on the wire and pays for it with a square
        /// root per decompressed point.
        pub fn verify_compressed(vk: Bytes, proof: Bytes, inputs: Bytes) -> bool {
            let vk = VerifyingKey::<Bn254>::deserialize_compressed(vk.as_slice()).expect("bad vk");
            let proof = Proof::<Bn254>::deserialize_compressed(proof.as_slice()).expect("bad proof");
            Groth16::<Bn254>::verify(&vk, &decode_inputs(inputs.as_slice()), &proof).expect("verify failed")
        }

        /// Unprepared verifying key, uncompressed encodings.
        pub fn verify_uncompressed(vk: Bytes, proof: Bytes, inputs: Bytes) -> bool {
            let vk = VerifyingKey::<Bn254>::deserialize_uncompressed(vk.as_slice()).expect("bad vk");
            let proof = Proof::<Bn254>::deserialize_uncompressed(proof.as_slice()).expect("bad proof");
            Groth16::<Bn254>::verify(&vk, &decode_inputs(inputs.as_slice()), &proof).expect("verify failed")
        }

        /// Prepared verifying key supplied per call, uncompressed encodings. Isolates the verify
        /// from the `prepare` pairing.
        pub fn verify_prepared(pvk: Bytes, proof: Bytes, inputs: Bytes) -> bool {
            let pvk = PreparedVerifyingKey::<Bn254>::deserialize_uncompressed(pvk.as_slice()).expect("bad pvk");
            let proof = Proof::<Bn254>::deserialize_uncompressed(proof.as_slice()).expect("bad proof");
            Groth16::<Bn254>::verify_with_processed_vk(&pvk, &decode_inputs(inputs.as_slice()), &proof)
                .expect("verify failed")
        }

        /// Best case a contract can reach: prepared key already in component state, so the call
        /// decodes only the proof and the public inputs.
        pub fn verify_stateful(&self, proof: Bytes, inputs: Bytes) -> bool {
            let pvk = PreparedVerifyingKey::<Bn254>::deserialize_uncompressed(self.pvk.as_slice()).expect("bad pvk");
            let proof = Proof::<Bn254>::deserialize_uncompressed(proof.as_slice()).expect("bad proof");
            Groth16::<Bn254>::verify_with_processed_vk(&pvk, &decode_inputs(inputs.as_slice()), &proof)
                .expect("verify failed")
        }
    }
}
