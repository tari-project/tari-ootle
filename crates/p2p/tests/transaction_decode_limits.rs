//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::ristretto::RistrettoSecretKey;
use tari_ootle_p2p::{decode_transaction_with_max_size, proto};
use tari_ootle_transaction::{Epoch, Instruction, MAX_TRANSACTION_INSTRUCTIONS, Transaction};

fn sealed(fee_instructions: usize, instructions: usize) -> Transaction {
    Transaction::builder_localnet(Epoch(1))
        .with_fee_instructions((0..fee_instructions).map(|_| Instruction::DropAllProofsInWorkspace))
        .with_instructions((0..instructions).map(|_| Instruction::DropAllProofsInWorkspace))
        .build_and_seal(&RistrettoSecretKey::from(1u64))
}

#[test]
fn an_instruction_list_at_the_limit_decodes() {
    let transaction = sealed(1, MAX_TRANSACTION_INSTRUCTIONS);
    let wire = proto::transaction::Transaction::from(&transaction);
    let decoded = Transaction::try_from(wire).unwrap();
    assert_eq!(decoded.calculate_id(), transaction.calculate_id());
}

#[test]
fn an_instruction_list_over_the_limit_does_not_decode() {
    let over = MAX_TRANSACTION_INSTRUCTIONS + 1;
    for transaction in [sealed(0, over), sealed(over, 0)] {
        let wire = proto::transaction::Transaction::from(&transaction);
        assert!(Transaction::try_from(wire).is_err());
    }
}

#[test]
fn a_transaction_over_the_byte_cap_is_refused_before_decoding() {
    let wire = proto::transaction::Transaction::from(&sealed(0, 16));
    let len = wire.bor_encoded.len();
    decode_transaction_with_max_size(&wire, len).unwrap();

    let err = decode_transaction_with_max_size(&wire, len - 1).unwrap_err();
    assert!(err.to_string().contains("maximum allowed"), "{err}");

    // Bytes that are not a transaction at all are refused on their length alone.
    let garbage = proto::transaction::Transaction {
        bor_encoded: vec![0xff; 64],
    };
    let err = decode_transaction_with_max_size(&garbage, 63).unwrap_err();
    assert!(err.to_string().contains("maximum allowed"), "{err}");
}
