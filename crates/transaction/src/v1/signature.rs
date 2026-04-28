//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use indexmap::IndexSet;
use ootle_byte_type::{ConvertFromByteType, FromByteType, ToByteType};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use tari_crypto::{
    keys::PublicKey as PublicKeyT,
    ristretto::{RistrettoPublicKey, RistrettoSchnorr, RistrettoSecretKey},
    tari_utilities,
    tari_utilities::ByteArray,
};
use tari_ootle_common_types::{Epoch, SubstateRequirement, signature::SignatureOutput};
use tari_template_lib_types::crypto::{RistrettoPublicKeyBytes, SchnorrSignatureBytes};

use crate::{
    BlobHashes,
    Instruction,
    UnsealedTransactionV1,
    UnsignedTransaction,
    UnsignedTransactionV1,
    hashing::transaction_hasher_v1,
    unsealed::UnsealedTransaction,
    v1::pruned::{PrunedUnsealedTransactionV1, PrunedUnsignedTransactionV1},
};

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TransactionSealSignature {
    public_key: RistrettoPublicKeyBytes,
    signature: SchnorrSignatureBytes,
}

impl TransactionSealSignature {
    pub fn new(public_key: RistrettoPublicKeyBytes, signature: SchnorrSignatureBytes) -> Self {
        Self { public_key, signature }
    }

    pub fn sign_v1(secret_key: &RistrettoSecretKey, transaction: &UnsealedTransactionV1) -> Self {
        let public_key = RistrettoPublicKey::from_secret_key(secret_key);

        let message = Self::create_message_v1(transaction);
        Self {
            signature: RistrettoSchnorr::sign(secret_key, message, &mut OsRng)
                .expect("sign is infallible with Ristretto keys")
                .to_byte_type(),
            public_key: public_key.to_byte_type(),
        }
    }

    pub fn verify(&self, transaction: &UnsealedTransaction) -> bool {
        match transaction {
            UnsealedTransaction::V1(t) => self.verify_v1(t),
        }
    }

    pub fn verify_v1(&self, transaction: &UnsealedTransactionV1) -> bool {
        let message = Self::create_message_v1(transaction);
        let Ok(public_key) = self.public_key.try_from_byte_type() else {
            return false;
        };
        let Ok(signature) = RistrettoSchnorr::convert_from_byte_type(&self.signature) else {
            return false;
        };
        signature.verify(&public_key, message)
    }

    pub fn signature(&self) -> &SchnorrSignatureBytes {
        &self.signature
    }

    pub fn public_key(&self) -> &RistrettoPublicKeyBytes {
        &self.public_key
    }

    pub fn to_ristretto_public_key(&self) -> Result<RistrettoPublicKey, tari_utilities::ByteArrayError> {
        RistrettoPublicKey::from_canonical_bytes(self.public_key.as_bytes())
    }

    pub fn create_message(transaction: &UnsealedTransaction) -> [u8; 64] {
        match transaction {
            UnsealedTransaction::V1(t) => Self::create_message_v1(t),
        }
    }

    pub fn create_message_v1(transaction: &UnsealedTransactionV1) -> [u8; 64] {
        let unsigned = transaction.unsigned_transaction();
        let blob_hashes = unsigned.blobs.hashes();
        Self::create_message_v1_inner(
            transaction.schema_version(),
            TransactionSignatureFields::from(unsigned),
            &blob_hashes,
            transaction.signatures(),
        )
    }

    /// Pruned-form seal message. Uses the stored `BlobHashes` and produces the same digest as
    /// the equivalent full form would.
    pub fn create_message_v1_pruned(transaction: &PrunedUnsealedTransactionV1) -> [u8; 64] {
        Self::create_message_v1_inner(
            transaction.schema_version(),
            TransactionSignatureFields::from(transaction.unsigned_transaction()),
            transaction.blob_hashes(),
            transaction.signatures(),
        )
    }

    fn create_message_v1_inner(
        schema_version: u16,
        fields: TransactionSignatureFields<'_>,
        blob_hashes: &BlobHashes,
        signatures: &[TransactionSignature],
    ) -> [u8; 64] {
        // Project explicitly so blob bytes never enter the digest — only their commitments do.
        transaction_hasher_v1("SealSignature")
            .chain(&schema_version)
            .chain(&fields)
            .chain(blob_hashes)
            .chain(signatures)
            .result()
    }

    pub fn verify_v1_pruned(&self, transaction: &PrunedUnsealedTransactionV1) -> bool {
        let message = Self::create_message_v1_pruned(transaction);
        let Ok(public_key) = self.public_key.try_from_byte_type() else {
            return false;
        };
        let Ok(signature) = RistrettoSchnorr::convert_from_byte_type(&self.signature) else {
            return false;
        };
        signature.verify(&public_key, message)
    }
}

impl From<SignatureOutput> for TransactionSealSignature {
    fn from(output: SignatureOutput) -> Self {
        Self {
            public_key: output.public_key.to_byte_type(),
            signature: output.signature.to_byte_type(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TransactionSignature {
    public_key: RistrettoPublicKeyBytes,
    signature: SchnorrSignatureBytes,
}

impl TransactionSignature {
    pub fn new(public_key: RistrettoPublicKeyBytes, signature: SchnorrSignatureBytes) -> Self {
        Self { public_key, signature }
    }

    pub fn sign(
        secret_key: &RistrettoSecretKey,
        seal_signer: &RistrettoPublicKeyBytes,
        transaction: &UnsignedTransaction,
    ) -> Self {
        match transaction {
            UnsignedTransaction::V1(v1) => Self::sign_v1(secret_key, seal_signer, v1),
        }
    }

    pub fn sign_v1(
        secret_key: &RistrettoSecretKey,
        seal_signer: &RistrettoPublicKeyBytes,
        transaction: &UnsignedTransactionV1,
    ) -> Self {
        let public_key = RistrettoPublicKey::from_secret_key(secret_key);
        let message = Self::create_message_v1(seal_signer, transaction);

        Self {
            signature: RistrettoSchnorr::sign(secret_key, message, &mut OsRng)
                .expect("sign is infallible with Ristretto keys")
                .to_byte_type(),
            public_key: public_key.to_byte_type(),
        }
    }

    pub fn verify_v1(&self, seal_signer: &RistrettoPublicKeyBytes, transaction: &UnsignedTransactionV1) -> bool {
        let message = Self::create_message_v1(seal_signer, transaction);
        let Ok(public_key) = self.public_key.try_from_byte_type() else {
            return false;
        };
        let Ok(signature) = RistrettoSchnorr::convert_from_byte_type(&self.signature) else {
            return false;
        };
        signature.verify(&public_key, message)
    }

    pub fn signature(&self) -> &SchnorrSignatureBytes {
        &self.signature
    }

    pub fn public_key(&self) -> &RistrettoPublicKeyBytes {
        &self.public_key
    }

    pub fn create_message(seal_signer: &RistrettoPublicKeyBytes, transaction: &UnsignedTransaction) -> [u8; 64] {
        match transaction {
            UnsignedTransaction::V1(v1) => Self::create_message_v1(seal_signer, v1),
        }
    }

    pub fn create_message_v1(seal_signer: &RistrettoPublicKeyBytes, transaction: &UnsignedTransactionV1) -> [u8; 64] {
        let blob_hashes = transaction.blobs.hashes();
        Self::create_message_v1_inner(
            seal_signer,
            transaction.schema_version(),
            TransactionSignatureFields::from(transaction),
            &blob_hashes,
        )
    }

    /// Pruned-form extra-signer message. Uses the stored `BlobHashes`.
    pub fn create_message_v1_pruned(
        seal_signer: &RistrettoPublicKeyBytes,
        transaction: &PrunedUnsignedTransactionV1,
        blob_hashes: &BlobHashes,
    ) -> [u8; 64] {
        Self::create_message_v1_inner(
            seal_signer,
            transaction.schema_version(),
            TransactionSignatureFields::from(transaction),
            blob_hashes,
        )
    }

    fn create_message_v1_inner(
        seal_signer: &RistrettoPublicKeyBytes,
        schema_version: u16,
        fields: TransactionSignatureFields<'_>,
        blob_hashes: &BlobHashes,
    ) -> [u8; 64] {
        transaction_hasher_v1("Signature")
            .chain(&schema_version)
            .chain(seal_signer)
            .chain(&fields)
            .chain(blob_hashes)
            .result()
    }

    pub fn verify_v1_pruned(
        &self,
        seal_signer: &RistrettoPublicKeyBytes,
        transaction: &PrunedUnsignedTransactionV1,
        blob_hashes: &BlobHashes,
    ) -> bool {
        let message = Self::create_message_v1_pruned(seal_signer, transaction, blob_hashes);
        let Ok(public_key) = self.public_key.try_from_byte_type() else {
            return false;
        };
        let Ok(signature) = RistrettoSchnorr::convert_from_byte_type(&self.signature) else {
            return false;
        };
        signature.verify(&public_key, message)
    }
}

impl From<SignatureOutput> for TransactionSignature {
    fn from(output: SignatureOutput) -> Self {
        Self {
            public_key: output.public_key.to_byte_type(),
            signature: output.signature.to_byte_type(),
        }
    }
}

/// Field-by-field projection of `UnsignedTransactionV1` used in the signing/id hashing domains.
///
/// Notably *does not* include `blobs` — raw blob bytes never enter signing/id digests. Blob
/// commitments are chained separately as a `BlobHashes` after these fields.
#[derive(Debug, Clone, borsh::BorshSerialize)]
pub(crate) struct TransactionSignatureFields<'a> {
    network: u8,
    fee_instructions: &'a [Instruction],
    instructions: &'a [Instruction],
    inputs: &'a IndexSet<SubstateRequirement>,
    min_epoch: Option<Epoch>,
    max_epoch: Option<Epoch>,
    is_seal_signer_authorized: bool,
    dry_run: bool,
}

impl<'a> From<&'a UnsignedTransactionV1> for TransactionSignatureFields<'a> {
    fn from(transaction: &'a UnsignedTransactionV1) -> Self {
        Self {
            network: transaction.network,
            fee_instructions: &transaction.fee_instructions,
            instructions: &transaction.instructions,
            inputs: &transaction.inputs,
            min_epoch: transaction.min_epoch,
            max_epoch: transaction.max_epoch,
            is_seal_signer_authorized: transaction.is_seal_signer_authorized,
            dry_run: transaction.dry_run,
        }
    }
}

/// Pruned-form projection. Field-by-field identical to the full-form `From` so the borsh
/// encoding (and therefore the digest) is byte-identical.
impl<'a> From<&'a PrunedUnsignedTransactionV1> for TransactionSignatureFields<'a> {
    fn from(transaction: &'a PrunedUnsignedTransactionV1) -> Self {
        Self {
            network: transaction.network,
            fee_instructions: &transaction.fee_instructions,
            instructions: &transaction.instructions,
            inputs: &transaction.inputs,
            min_epoch: transaction.min_epoch,
            max_epoch: transaction.max_epoch,
            is_seal_signer_authorized: transaction.is_seal_signer_authorized,
            dry_run: transaction.dry_run,
        }
    }
}

#[cfg(test)]
mod tests {
    use tari_crypto::keys::SecretKey;
    use tari_engine_types::substate::SubstateId;
    use tari_template_lib_types::ComponentAddress;

    use super::*;

    fn sample_seal_signer() -> RistrettoPublicKeyBytes {
        RistrettoPublicKey::from_secret_key(&RistrettoSecretKey::random(&mut OsRng)).to_byte_type()
    }

    fn sample_unsigned() -> UnsignedTransactionV1 {
        let mut inputs = IndexSet::new();
        inputs.insert(SubstateRequirement::versioned(
            SubstateId::Component(ComponentAddress::from_array([1; 32])),
            1,
        ));
        inputs.insert(SubstateRequirement::versioned(
            SubstateId::Component(ComponentAddress::from_array([2; 32])),
            2,
        ));
        UnsignedTransactionV1 {
            network: 42,
            fee_instructions: vec![Instruction::DropAllProofsInWorkspace],
            instructions: vec![
                Instruction::DropAllProofsInWorkspace,
                Instruction::PutLastInstructionOutputOnWorkspace { key: 7 },
            ],
            inputs,
            min_epoch: Some(Epoch(100)),
            max_epoch: Some(Epoch(200)),
            is_seal_signer_authorized: false,
            dry_run: true,
            blobs: crate::Blobs::empty(),
        }
    }

    fn random_signature(tx: &UnsignedTransactionV1, seal_signer: &RistrettoPublicKeyBytes) -> TransactionSignature {
        let sk = RistrettoSecretKey::random(&mut OsRng);
        TransactionSignature::sign_v1(&sk, seal_signer, tx)
    }

    fn sig_msg(seal_signer: &RistrettoPublicKeyBytes, tx: &UnsignedTransactionV1) -> [u8; 64] {
        TransactionSignature::create_message_v1(seal_signer, tx)
    }

    fn seal_msg(t: &UnsealedTransactionV1) -> [u8; 64] {
        TransactionSealSignature::create_message_v1(t)
    }

    #[test]
    fn signature_message_is_deterministic() {
        let signer = sample_seal_signer();
        let tx = sample_unsigned();
        assert_eq!(sig_msg(&signer, &tx), sig_msg(&signer, &tx));
    }

    /// Every field of the signed message (seal signer + every field of UnsignedTransactionV1) must
    /// influence the digest. A failure here means a tx field has escaped the signing domain and
    /// signatures are malleable with respect to it.
    #[test]
    fn signature_message_binds_all_fields() {
        let signer = sample_seal_signer();
        let base = sample_unsigned();
        let base_msg = sig_msg(&signer, &base);

        // seal_signer context
        let other_signer = sample_seal_signer();
        assert_ne!(sig_msg(&other_signer, &base), base_msg, "seal_signer");

        // network
        let mut tx = base.clone();
        tx.network = tx.network.wrapping_add(1);
        assert_ne!(sig_msg(&signer, &tx), base_msg, "network");

        // fee_instructions: extra / empty
        let mut tx = base.clone();
        tx.fee_instructions.push(Instruction::DropAllProofsInWorkspace);
        assert_ne!(sig_msg(&signer, &tx), base_msg, "fee_instructions (extra)");
        let mut tx = base.clone();
        tx.fee_instructions.clear();
        assert_ne!(sig_msg(&signer, &tx), base_msg, "fee_instructions (empty)");

        // instructions: extra / reordered
        let mut tx = base.clone();
        tx.instructions.push(Instruction::DropAllProofsInWorkspace);
        assert_ne!(sig_msg(&signer, &tx), base_msg, "instructions (extra)");
        let mut tx = base.clone();
        tx.instructions.reverse();
        assert_ne!(sig_msg(&signer, &tx), base_msg, "instructions (reordered)");

        // inputs: extra / reorder / version changed
        let mut tx = base.clone();
        tx.inputs.insert(SubstateRequirement::versioned(
            SubstateId::Component(ComponentAddress::from_array([9; 32])),
            1,
        ));
        assert_ne!(sig_msg(&signer, &tx), base_msg, "inputs (extra)");

        let mut tx = base.clone();
        tx.inputs = tx.inputs.iter().rev().cloned().collect();
        assert_ne!(sig_msg(&signer, &tx), base_msg, "inputs (reordered)");

        let mut tx = base.clone();
        tx.inputs = base
            .inputs
            .iter()
            .map(|i| SubstateRequirement {
                substate_id: i.substate_id.clone(),
                version: i.version.map(|v| v.wrapping_add(1)),
            })
            .collect();
        assert_ne!(sig_msg(&signer, &tx), base_msg, "inputs (version changed)");

        // min_epoch: value change / Some <-> None
        let mut tx = base.clone();
        tx.min_epoch = Some(Epoch(101));
        assert_ne!(sig_msg(&signer, &tx), base_msg, "min_epoch (value)");
        let mut tx = base.clone();
        tx.min_epoch = None;
        assert_ne!(sig_msg(&signer, &tx), base_msg, "min_epoch (None)");

        // max_epoch: value change / Some <-> None
        let mut tx = base.clone();
        tx.max_epoch = Some(Epoch(999));
        assert_ne!(sig_msg(&signer, &tx), base_msg, "max_epoch (value)");
        let mut tx = base.clone();
        tx.max_epoch = None;
        assert_ne!(sig_msg(&signer, &tx), base_msg, "max_epoch (None)");

        // is_seal_signer_authorized
        let mut tx = base.clone();
        tx.is_seal_signer_authorized = !tx.is_seal_signer_authorized;
        assert_ne!(sig_msg(&signer, &tx), base_msg, "is_seal_signer_authorized");

        // dry_run
        let mut tx = base.clone();
        tx.dry_run = !tx.dry_run;
        assert_ne!(sig_msg(&signer, &tx), base_msg, "dry_run");

        // blobs: payload contents must influence the digest via per-blob commitments
        let mut tx = base.clone();
        tx.blobs.push(crate::Blob::from(vec![1, 2, 3])).unwrap();
        assert_ne!(sig_msg(&signer, &tx), base_msg, "blobs (added)");

        // Two transactions with the same blob count but different bytes must differ.
        let mut a = base.clone();
        a.blobs.push(crate::Blob::from(vec![1])).unwrap();
        let mut b = base.clone();
        b.blobs.push(crate::Blob::from(vec![2])).unwrap();
        assert_ne!(sig_msg(&signer, &a), sig_msg(&signer, &b), "blobs (contents)");
    }

    #[test]
    fn signature_is_bound_to_seal_signer_context() {
        let signer_sk = RistrettoSecretKey::random(&mut OsRng);
        let seal_signer_pk = sample_seal_signer();
        let other_seal_signer_pk = sample_seal_signer();
        let tx = sample_unsigned();

        let sig = TransactionSignature::sign_v1(&signer_sk, &seal_signer_pk, &tx);
        assert!(sig.verify_v1(&seal_signer_pk, &tx));
        assert!(
            !sig.verify_v1(&other_seal_signer_pk, &tx),
            "a signature made under one seal signer must not verify under another",
        );

        let mut mutated = tx.clone();
        mutated.dry_run = !mutated.dry_run;
        assert!(!sig.verify_v1(&seal_signer_pk, &mutated));
    }

    fn unsealed_with(unsigned: UnsignedTransactionV1, sigs: Vec<TransactionSignature>) -> UnsealedTransactionV1 {
        UnsealedTransactionV1::new(unsigned, sigs)
    }

    #[test]
    fn seal_message_is_deterministic() {
        let seal_signer = sample_seal_signer();
        let unsigned = sample_unsigned();
        let sig = random_signature(&unsigned, &seal_signer);
        let a = unsealed_with(unsigned.clone(), vec![sig.clone()]);
        let b = unsealed_with(unsigned, vec![sig]);
        assert_eq!(seal_msg(&a), seal_msg(&b));
    }

    /// Every field of UnsignedTransactionV1 reached via the seal message must influence the digest.
    #[test]
    fn seal_message_binds_all_unsigned_fields() {
        let seal_signer = sample_seal_signer();
        let base_unsigned = sample_unsigned();
        let sigs = vec![random_signature(&base_unsigned, &seal_signer)];
        let base = unsealed_with(base_unsigned.clone(), sigs.clone());
        let base_msg = seal_msg(&base);

        let with_body = |u: UnsignedTransactionV1| unsealed_with(u, sigs.clone());

        // network
        let mut u = base_unsigned.clone();
        u.network = u.network.wrapping_add(1);
        assert_ne!(seal_msg(&with_body(u)), base_msg, "network");

        // fee_instructions
        let mut u = base_unsigned.clone();
        u.fee_instructions.push(Instruction::DropAllProofsInWorkspace);
        assert_ne!(seal_msg(&with_body(u)), base_msg, "fee_instructions");

        // instructions: extra / reordered
        let mut u = base_unsigned.clone();
        u.instructions.push(Instruction::DropAllProofsInWorkspace);
        assert_ne!(seal_msg(&with_body(u)), base_msg, "instructions (extra)");
        let mut u = base_unsigned.clone();
        u.instructions.reverse();
        assert_ne!(seal_msg(&with_body(u)), base_msg, "instructions (reordered)");

        // inputs: extra / reordered / version changed
        let mut u = base_unsigned.clone();
        u.inputs.insert(SubstateRequirement::versioned(
            SubstateId::Component(ComponentAddress::from_array([9; 32])),
            1,
        ));
        assert_ne!(seal_msg(&with_body(u)), base_msg, "inputs (extra)");

        let mut u = base_unsigned.clone();
        u.inputs = u.inputs.iter().rev().cloned().collect();
        assert_ne!(seal_msg(&with_body(u)), base_msg, "inputs (reordered)");

        let mut u = base_unsigned.clone();
        u.inputs = base_unsigned
            .inputs
            .iter()
            .map(|i| SubstateRequirement {
                substate_id: i.substate_id.clone(),
                version: i.version.map(|v| v.wrapping_add(1)),
            })
            .collect();
        assert_ne!(seal_msg(&with_body(u)), base_msg, "inputs (version changed)");

        // min_epoch
        let mut u = base_unsigned.clone();
        u.min_epoch = Some(Epoch(101));
        assert_ne!(seal_msg(&with_body(u)), base_msg, "min_epoch (value)");
        let mut u = base_unsigned.clone();
        u.min_epoch = None;
        assert_ne!(seal_msg(&with_body(u)), base_msg, "min_epoch (None)");

        // max_epoch
        let mut u = base_unsigned.clone();
        u.max_epoch = Some(Epoch(999));
        assert_ne!(seal_msg(&with_body(u)), base_msg, "max_epoch (value)");
        let mut u = base_unsigned.clone();
        u.max_epoch = None;
        assert_ne!(seal_msg(&with_body(u)), base_msg, "max_epoch (None)");

        // is_seal_signer_authorized
        let mut u = base_unsigned.clone();
        u.is_seal_signer_authorized = !u.is_seal_signer_authorized;
        assert_ne!(seal_msg(&with_body(u)), base_msg, "is_seal_signer_authorized");

        // dry_run
        let mut u = base_unsigned.clone();
        u.dry_run = !u.dry_run;
        assert_ne!(seal_msg(&with_body(u)), base_msg, "dry_run");

        // blobs: changes to the payload must alter the seal digest via per-blob commitments
        let mut u = base_unsigned.clone();
        u.blobs.push(crate::Blob::from(vec![9, 9, 9])).unwrap();
        assert_ne!(seal_msg(&with_body(u)), base_msg, "blobs (added)");
    }

    /// The seal signature binds the prior signatures — their presence, content and order must all
    /// affect the seal digest, otherwise a seal could be lifted onto a transaction with forged
    /// signatures.
    #[test]
    fn seal_message_binds_signatures() {
        let seal_signer = sample_seal_signer();
        let unsigned = sample_unsigned();

        let sig1 = random_signature(&unsigned, &seal_signer);
        let sig2 = random_signature(&unsigned, &seal_signer);

        let no_sigs = unsealed_with(unsigned.clone(), vec![]);
        let with_sig1 = unsealed_with(unsigned.clone(), vec![sig1.clone()]);
        let with_sig2 = unsealed_with(unsigned.clone(), vec![sig2.clone()]);
        let ab = unsealed_with(unsigned.clone(), vec![sig1.clone(), sig2.clone()]);
        let ba = unsealed_with(unsigned, vec![sig2, sig1]);

        assert_ne!(seal_msg(&no_sigs), seal_msg(&with_sig1), "count (0 vs 1)");
        assert_ne!(seal_msg(&with_sig1), seal_msg(&with_sig2), "content");
        assert_ne!(seal_msg(&with_sig1), seal_msg(&ab), "count (1 vs 2)");
        assert_ne!(seal_msg(&ab), seal_msg(&ba), "order");
    }

    #[test]
    fn seal_signature_roundtrip() {
        let sealer_sk = RistrettoSecretKey::random(&mut OsRng);
        let seal_signer_pk: RistrettoPublicKeyBytes = RistrettoPublicKey::from_secret_key(&sealer_sk).to_byte_type();

        let unsigned = sample_unsigned();
        let sig = random_signature(&unsigned, &seal_signer_pk);
        let t = unsealed_with(unsigned, vec![sig]);

        let seal = TransactionSealSignature::sign_v1(&sealer_sk, &t);
        assert!(seal.verify_v1(&t));

        // Mutating a body field breaks the seal.
        let mut mutated_inner = t.unsigned_transaction().clone();
        mutated_inner.dry_run = !mutated_inner.dry_run;
        let mutated = UnsealedTransactionV1::new(mutated_inner, t.signatures().to_vec());
        assert!(!seal.verify_v1(&mutated));

        // Mutating signatures also breaks the seal.
        let extra_sig = random_signature(t.unsigned_transaction(), &seal_signer_pk);
        let mut sigs = t.signatures().to_vec();
        sigs.push(extra_sig);
        let mutated = UnsealedTransactionV1::new(t.unsigned_transaction().clone(), sigs);
        assert!(!seal.verify_v1(&mutated));
    }
}
