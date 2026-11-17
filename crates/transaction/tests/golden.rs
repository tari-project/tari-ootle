//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Golden digests for one fixed transaction under each schema.
//!
//! The V1 values pin the V1 rules, which existing V1 transactions depend on to verify and to keep
//! their ids. The V2 values pin the current rules for clients that compute ids and signing messages
//! themselves.

use tari_ootle_transaction::{
    AuthorizationContext,
    AuthorizerSet,
    Epoch,
    Instruction,
    PrunedTransaction,
    Sr25519SignatureBytes,
    Transaction,
    TransactionIntent,
    TransactionSealSignature,
    TransactionSignature,
    TransactionV1,
    UnsealedTransaction,
    UnsealedTransactionV1,
    UnsignedTransaction,
    UnsignedTransactionV1,
};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

fn key(byte: u8) -> RistrettoPublicKeyBytes {
    RistrettoPublicKeyBytes::from_bytes(&[byte; 32]).unwrap()
}

fn body() -> UnsignedTransactionV1 {
    UnsignedTransactionV1::new(
        1u8,
        vec![Instruction::DropAllProofsInWorkspace],
        vec![Instruction::PutLastInstructionOutputOnWorkspace { key: 3 }],
        Default::default(),
        None,
        Epoch(10),
        false,
    )
}

/// Two authorizations with fixed bytes, listed in descending key order. Neither verifies; the
/// digests do not depend on that.
fn signatures() -> Vec<TransactionSignature> {
    vec![
        TransactionSignature::new(key(5), Sr25519SignatureBytes::from_array([6; 64])),
        TransactionSignature::new(key(3), Sr25519SignatureBytes::from_array([4; 64])),
    ]
}

fn seal() -> TransactionSealSignature {
    TransactionSealSignature::new(key(2), Sr25519SignatureBytes::from_array([7; 64]))
}

struct Digests {
    authorization: String,
    seal: String,
    id: String,
    intent: String,
}

fn digests(unsigned: UnsignedTransaction, unsealed: UnsealedTransaction) -> Digests {
    let authorizers = AuthorizerSet::from_signatures(&signatures());
    let authorization =
        TransactionSignature::create_message(AuthorizationContext::new(&key(2), &authorizers), &unsigned);
    let seal_message = TransactionSealSignature::create_message(&unsealed);
    let tx = Transaction::new(unsealed, seal());
    assert_eq!(tx.calculate_id(), PrunedTransaction::from(tx.clone()).calculate_id());
    Digests {
        authorization: hex::encode(authorization.as_bytes()),
        seal: hex::encode(seal_message.as_bytes()),
        id: tx.calculate_id().to_string(),
        intent: tx.calculate_intent_commitment().to_string(),
    }
}

#[test]
fn v1_digests_are_pinned() {
    let d = digests(
        UnsignedTransaction::V1(body()),
        UnsealedTransaction::V1(UnsealedTransactionV1::new(body(), signatures())),
    );
    assert_eq!(
        d.authorization,
        "29a6b709a55aa4ddfe8e064c1c7c0fe119cc26680272241d374f7f6a7194df28950fb5c8b1a8ba91f5c5a11187983ce311659440da8feb9f8739f7576f435c06"
    );
    assert_eq!(
        d.seal,
        "e11febf608a2ca0087744df2f6c5f9bd81c5923a448e01fa75c808d7682eeb3d5354de050f16e65be0e82648bbe8d5d9b171c562f310c849d5fa1c713245ce72"
    );
    assert_eq!(d.id, "8fdc6d3eeb8406193b65d38b24c06dd4090d566b20cacdf928365efbe551b90b");
    assert_eq!(
        d.intent,
        "a9fd04796cd2a1d9704c5bd9d796735d3d241e43597052cf64136d5f2459b80e"
    );
}

#[test]
fn v2_digests_are_pinned() {
    let d = digests(
        UnsignedTransaction::V2(body()),
        UnsealedTransaction::V2(UnsealedTransactionV1::with_canonical_signatures(body(), signatures())),
    );
    assert_eq!(
        d.authorization,
        "0251dce56c24ee007ecf54170f1ee95db823dcd91f04e2365e673b9951a2822bc4d9e4aee724b68a25c0f348bd86cee6f15434cb0984e9c629d6e37f71154cbd"
    );
    assert_eq!(
        d.seal,
        "c0e5e5f62aa290429f51d78f2b3f0a981f5366d123e232d3a656daabd2ec40287b3b95a165c5b178a097d7cfd1039db02520ed2b24de67249bac040e1254f884"
    );
    assert_eq!(d.id, "d5d1d9ddbe825fcd42db503a92328360ced66156b41153fa8e8de4e8c447fec8");
    assert_eq!(
        d.intent,
        "fd1bf4d9510a9b638171c6861f05bb1b631d2c385fce25201cbd530491441c0b"
    );
}

/// The body encoding is shared, so only the variant tag separates a V1 transaction from a V2 one on
/// the wire, and each decodes back to its own schema.
#[test]
fn each_schema_round_trips_through_cbor() {
    for tx in [
        Transaction::V1(TransactionV1::new(
            UnsealedTransactionV1::new(body(), signatures()),
            seal(),
        )),
        Transaction::V2(TransactionV1::new(
            UnsealedTransactionV1::with_canonical_signatures(body(), signatures()),
            seal(),
        )),
    ] {
        let decoded: Transaction = tari_bor::decode(&tari_bor::encode(&tx).unwrap()).unwrap();
        assert_eq!(decoded.schema(), tx.schema());
        assert_eq!(decoded.calculate_id(), tx.calculate_id());
    }
}
