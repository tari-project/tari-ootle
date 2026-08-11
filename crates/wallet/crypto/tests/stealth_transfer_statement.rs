//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::iter;

use tari_crypto::{
    keys::{PublicKey, SecretKey},
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
use tari_engine_types::{Hash64, crypto::messages, stealth};
use tari_ootle_common_types::crypto::create_key_pair_from_seed;
use tari_ootle_wallet_crypto::{
    MaskAndValue,
    OutputWitness,
    StealthInputWitness,
    StealthOutputWitness,
    confidential,
    stealth::create_transfer_statement,
};
use tari_template_lib_types::{
    Amount,
    EncryptedData,
    access_rules::AccessRule,
    bytes::Bytes,
    crypto::{PedersenCommitmentBytes, UtxoTag},
    stealth::{
        AtomicCondition,
        MerkleProof,
        SpendAuthorization,
        SpendCondition,
        SpendWitness,
        StealthInputsStatement,
        StealthOutputsStatement,
        StealthTransferStatement,
    },
};

#[test]
fn it_create_a_valid_revealed_only_proof() {
    let proof = confidential::create_withdraw_proof(
        &[],
        Amount::from(123u64),
        None,
        Amount::from(123u64),
        None,
        Amount::from(0u64),
    )
    .unwrap();

    assert!(proof.is_revealed_only());
}

mod stealth_tests {
    use ootle_byte_type::ToByteType;

    use super::*;

    #[test]
    fn it_errors_for_noop_transfer() {
        let statement =
            create_transfer_statement(iter::empty(), Amount::zero(), iter::empty(), Amount::zero()).unwrap();
        stealth::validate_transfer(&statement, None).unwrap_err();
    }

    #[test]
    fn it_creates_a_valid_statement() {
        let inputs = make_input_statements(&[(1, 1000), (2, 2000), (3, 3000)]);
        let revealed_input_amount = Amount::zero();

        let output_statements = make_output_statements(&[6000]);
        let revealed_output_amount = Amount::from(0u64);

        let statement = create_transfer_statement(
            inputs,
            revealed_input_amount,
            output_statements.iter(),
            revealed_output_amount,
        )
        .unwrap();

        stealth::validate_transfer(&statement, None).unwrap();
    }

    #[test]
    fn it_creates_a_valid_statement_with_revealed() {
        let inputs = make_input_statements(&[(1, 1000), (2, 2000), (3, 3000)]);
        let revealed_input_amount = Amount::from(6000u64);

        let output_statements = make_output_statements(&[100, 200, 300]);
        let revealed_output_amount = Amount::from_u64(6000 + 6000 - 100 - 200 - 300);

        let statement = create_transfer_statement(
            inputs,
            revealed_input_amount,
            output_statements.iter(),
            revealed_output_amount,
        )
        .unwrap();

        stealth::validate_transfer(&statement, None).unwrap();
    }

    #[test]
    fn it_creates_a_valid_statement_with_revealed_only() {
        let revealed_input_amount = Amount::from(6000u64);
        let revealed_output_amount = Amount::from(6000u64);
        let statement = create_transfer_statement(
            iter::empty(),
            revealed_input_amount,
            iter::empty(),
            revealed_output_amount,
        )
        .unwrap();
        stealth::validate_transfer(&statement, None).unwrap();

        let revealed_input_amount = Amount::from(6000u64);
        let revealed_output_amount = Amount::from(5999u64);
        let statement = create_transfer_statement(
            iter::empty(),
            revealed_input_amount,
            iter::empty(),
            revealed_output_amount,
        )
        .unwrap();
        stealth::validate_transfer(&statement, None).unwrap_err(); // Invalid, output is less than input
    }

    /// The balance proof's challenge covers the output authorisation, so an intercepted statement cannot be
    /// re-targeted at another spend key: `auth` does not enter the excess, and the commitments are untouched.
    #[test]
    fn balance_proof_binds_output_auth() {
        let mut statement = valid_statement();
        let (_, attacker_pk) = create_key_pair_from_seed(99);
        statement.outputs_statement.outputs[0].auth = SpendAuthorization::Key(attacker_pk.to_byte_type());

        stealth::validate_transfer(&statement, None).unwrap_err();
    }

    #[test]
    fn balance_proof_binds_output_tag() {
        let mut statement = valid_statement();
        statement.outputs_statement.outputs[0].tag = UtxoTag::new(7);

        stealth::validate_transfer(&statement, None).unwrap_err();
    }

    /// Rewriting the sender nonce leaves the recipient unable to derive the decryption key for the output's
    /// encrypted mask and value, so it must invalidate the proof even though no committed value changes.
    #[test]
    fn balance_proof_binds_sender_public_nonce() {
        let mut statement = valid_statement();
        let (_, other_nonce) = create_key_pair_from_seed(98);
        statement.outputs_statement.outputs[0].output.sender_public_nonce = other_nonce.to_byte_type();

        stealth::validate_transfer(&statement, None).unwrap_err();
    }

    #[test]
    fn balance_proof_binds_input_witness() {
        let mut statement = valid_statement();
        statement.inputs_statement.inputs[0].witness = SpendWitness::ScriptPath {
            leaf: SpendCondition::single(AtomicCondition::AccessRule(AccessRule::AllowAll)),
            proof: MerkleProof::empty(),
            data: Bytes::default(),
        };

        stealth::validate_transfer(&statement, None).unwrap_err();
    }

    /// The auditor's path: a record holding the commitments, the minimum value promises, the revealed amounts and
    /// the 32-byte aux digest must reproduce the challenge the signer committed to. If these ever diverge, a
    /// retained proof stops being re-verifiable.
    #[test]
    fn challenge_reconstructs_from_retained_parts() {
        let statement = valid_statement();
        let inputs = &statement.inputs_statement;
        let outputs = &statement.outputs_statement;

        let (_, excess) = create_key_pair_from_seed(11);
        let (_, nonce) = create_key_pair_from_seed(12);

        assert_eq!(
            messages::stealth_balance_proof64(&excess, &nonce, inputs, outputs),
            challenge_from_record(&excess, &nonce, inputs, outputs, &record_outputs(outputs))
        );
    }

    /// `minimum_value_promise` is bound by the explicit half, not by the aux digest.
    ///
    /// A record carries the promise separately from the digest, so the threat is a record that alters the promise
    /// while replaying the digest verbatim. Were the promise bound only inside the digest, the altered value would
    /// contribute nothing and the challenge would still reproduce.
    #[test]
    fn challenge_binds_minimum_value_promise_outside_the_aux_digest() {
        let statement = valid_statement();
        let inputs = &statement.inputs_statement;
        let outputs = &statement.outputs_statement;

        let (_, excess) = create_key_pair_from_seed(13);
        let (_, nonce) = create_key_pair_from_seed(14);

        let mut tampered = record_outputs(outputs);
        tampered[0].1 += 1;

        assert_ne!(
            messages::stealth_balance_proof64(&excess, &nonce, inputs, outputs),
            challenge_from_record(&excess, &nonce, inputs, outputs, &tampered)
        );
    }

    /// The `(commitment, minimum_value_promise)` pairs a retained record carries for these outputs.
    fn record_outputs(outputs: &StealthOutputsStatement) -> Vec<(PedersenCommitmentBytes, u64)> {
        outputs
            .outputs
            .iter()
            .map(|o| (*o.commitment(), o.output.minimum_value_promise))
            .collect()
    }

    /// Rebuilds the challenge the way a verifier holding a retained record does: the value fields come from
    /// `record_outputs`, while the aux digest is replayed as the opaque 32 bytes the record stored.
    fn challenge_from_record(
        excess: &RistrettoPublicKey,
        nonce: &RistrettoPublicKey,
        inputs: &StealthInputsStatement,
        outputs: &StealthOutputsStatement,
        record_outputs: &[(PedersenCommitmentBytes, u64)],
    ) -> Hash64 {
        let record_inputs = inputs.inputs.iter().map(|i| i.commitment).collect::<Vec<_>>();

        messages::stealth_balance_proof64_from_parts(excess, nonce, &messages::StealthBalanceProofParts {
            input_commitments: record_inputs.iter(),
            revealed_input_amount: &inputs.revealed_amount,
            outputs: record_outputs
                .iter()
                .map(|(commitment, minimum_value_promise)| messages::StealthOutputBinding {
                    commitment,
                    minimum_value_promise: *minimum_value_promise,
                }),
            revealed_output_amount: &outputs.revealed_output_amount,
            aux_digest: &messages::stealth_balance_proof_aux32(inputs, outputs),
        })
    }

    fn valid_statement() -> StealthTransferStatement {
        let statement = create_transfer_statement(
            make_input_statements(&[(1, 1000), (2, 2000)]),
            Amount::zero(),
            make_output_statements(&[1000, 2000]).iter(),
            Amount::zero(),
        )
        .unwrap();
        stealth::validate_transfer(&statement, None).expect("statement must be valid before tampering");
        statement
    }

    fn make_input_statements(amounts: &[(u8, u64)]) -> Vec<StealthInputWitness> {
        amounts
            .iter()
            .map(|&(seed, amount)| {
                let (mask, _) = create_key_pair_from_seed(seed);
                StealthInputWitness::new(MaskAndValue::new(amount, mask.clone()))
            })
            .collect()
    }

    fn make_output_statements(amounts: &[u64]) -> Vec<StealthOutputWitness> {
        amounts
            .iter()
            .filter(|amount| **amount > 0)
            .map(|&amount| {
                let output_mask = RistrettoSecretKey::random(&mut rand::rng());
                // For testing purposes, we use the mask as the owner key
                let output_owner_public_key = RistrettoPublicKey::from_secret_key(&output_mask);
                let statement = OutputWitness {
                    amount,
                    mask: output_mask,
                    resource_view_key: None,
                    // This is client/wallet on-chain data and not required for spending in tests
                    sender_public_nonce: {
                        let (_sk, pk) = create_key_pair_from_seed(0);
                        pk
                    },
                    minimum_value_promise: 0,
                    encrypted_data: EncryptedData::empty(),
                };

                StealthOutputWitness {
                    witness: statement,
                    auth: SpendAuthorization::Key(output_owner_public_key.to_byte_type()),
                    tag: UtxoTag::new(0),
                }
            })
            .collect()
    }
}
