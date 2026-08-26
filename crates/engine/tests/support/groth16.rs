//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Groth16/BN254 setup, proving and encoding for the `zk_verifier` template, shared by
//! `tests/zk_verify.rs` and `examples/zk_points_calibrate.rs`. Each consumer uses a subset of the
//! encodings, hence the blanket `dead_code` allowance.

#![allow(dead_code)]

use ark_bn254::{Bn254, Fr};
use ark_groth16::{Groth16, prepare_verifying_key};
use ark_relations::{
    lc,
    r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError},
};
use ark_serialize::CanonicalSerialize;
use ark_snark::SNARK;
use ark_std::{
    UniformRand,
    rand::{SeedableRng, rngs::StdRng},
};
use tari_template_lib::types::bytes::Bytes;

/// R1CS constraints in the test circuit. Verification cost is independent of circuit size — only of
/// the public input count — so this only has to be large enough to be a real proof.
pub const CONSTRAINTS: usize = 64;

/// A circuit with `num_public` public inputs, each constrained to the same witness product. The
/// shape is irrelevant to verification cost, so this is the cheapest circuit that produces a valid
/// proof with a chosen public input count.
#[derive(Clone)]
pub struct InputCountCircuit {
    pub a: Fr,
    pub b: Fr,
    pub num_public: usize,
    pub num_constraints: usize,
}

impl ConstraintSynthesizer<Fr> for InputCountCircuit {
    fn generate_constraints(self, cs: ConstraintSystemRef<Fr>) -> Result<(), SynthesisError> {
        let a = cs.new_witness_variable(|| Ok(self.a))?;
        let b = cs.new_witness_variable(|| Ok(self.b))?;
        let product = self.a * self.b;
        for _ in 0..self.num_public {
            let c = cs.new_input_variable(|| Ok(product))?;
            cs.enforce_constraint(lc!() + a, lc!() + b, lc!() + c)?;
        }
        let filler = cs.new_witness_variable(|| Ok(product))?;
        for _ in self.num_public..self.num_constraints {
            cs.enforce_constraint(lc!() + a, lc!() + b, lc!() + filler)?;
        }
        Ok(())
    }
}

/// One circuit's setup and proof, in every encoding the template accepts.
///
/// Held as [`Bytes`] rather than `Vec<u8>` because that is what the template's arguments are:
/// `Bytes` encodes as a CBOR byte string, where a `Vec<u8>` argument would encode as an array of
/// integers and make the callee pay to decode one CBOR item per byte — which for the 34 KiB
/// prepared key costs more than a tenth of the verification itself.
pub struct Fixture {
    pub vk_compressed: Bytes,
    pub vk_uncompressed: Bytes,
    pub pvk_uncompressed: Bytes,
    pub proof_compressed: Bytes,
    pub proof_uncompressed: Bytes,
    pub inputs: Bytes,
    /// Public inputs the proof does not attest to. Verification returns `false` for these rather
    /// than erroring, so they exercise the rejection path without also exercising deserialization
    /// failure.
    pub wrong_inputs: Bytes,
}

/// Builds a verifying key and a valid proof for a circuit with `num_public` public inputs.
///
/// Deterministic: the seed is fixed so the fixture — and therefore the metering points a template
/// spends verifying it — is identical from run to run.
pub fn fixture(num_public: usize) -> Fixture {
    let rng = &mut StdRng::from_seed([42u8; 32]);
    let circuit = InputCountCircuit {
        a: Fr::rand(rng),
        b: Fr::rand(rng),
        num_public,
        num_constraints: CONSTRAINTS.max(num_public),
    };

    let (pk, vk) = Groth16::<Bn254>::circuit_specific_setup(circuit.clone(), rng).expect("setup");
    let proof = Groth16::<Bn254>::prove(&pk, circuit.clone(), rng).expect("prove");
    let public_inputs = vec![circuit.a * circuit.b; num_public];

    // A measurement taken on a rejection path would miss most of the verification, so the fixture
    // has to be known-good before the template ever sees it.
    assert!(
        Groth16::<Bn254>::verify(&vk, &public_inputs, &proof).expect("verify"),
        "fixture proof must verify natively"
    );

    let pvk = prepare_verifying_key(&vk);
    Fixture {
        vk_compressed: ser_compressed(&vk),
        vk_uncompressed: ser_uncompressed(&vk),
        pvk_uncompressed: ser_uncompressed(&pvk),
        proof_compressed: ser_compressed(&proof),
        proof_uncompressed: ser_uncompressed(&proof),
        inputs: ser_uncompressed(&public_inputs),
        wrong_inputs: ser_uncompressed(&vec![circuit.a * circuit.b + Fr::from(1u64); num_public]),
    }
}

pub fn ser_compressed<T: CanonicalSerialize>(value: &T) -> Bytes {
    let mut bytes = Vec::new();
    value.serialize_compressed(&mut bytes).expect("serialize");
    bytes.into()
}

pub fn ser_uncompressed<T: CanonicalSerialize>(value: &T) -> Bytes {
    let mut bytes = Vec::new();
    value.serialize_uncompressed(&mut bytes).expect("serialize");
    bytes.into()
}

/// Verifies natively, timed. Used to express what the same check costs outside the WASM meter.
pub fn native_verify_ms(num_public: usize, trials: usize) -> f64 {
    use std::time::Instant;

    let rng = &mut StdRng::from_seed([42u8; 32]);
    let circuit = InputCountCircuit {
        a: Fr::rand(rng),
        b: Fr::rand(rng),
        num_public,
        num_constraints: CONSTRAINTS.max(num_public),
    };
    let (pk, vk) = Groth16::<Bn254>::circuit_specific_setup(circuit.clone(), rng).expect("setup");
    let proof = Groth16::<Bn254>::prove(&pk, circuit.clone(), rng).expect("prove");
    let inputs = vec![circuit.a * circuit.b; num_public];
    let pvk = prepare_verifying_key(&vk);

    // The minimum over the trials is the estimator, matching `native_points_calibrate`: a scheduler
    // interruption can only make a sample slower, so the fastest is the least contaminated.
    let mut best = f64::MAX;
    for _ in 0..trials {
        let start = Instant::now();
        let ok = Groth16::<Bn254>::verify_with_processed_vk(&pvk, &inputs, &proof).expect("verify");
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        assert!(ok, "proof must verify");
        best = best.min(elapsed);
    }
    best
}
