//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashSet;

use indexmap::IndexSet;
use tari_crypto::ristretto::RistrettoSecretKey;
use tari_engine_types::{indexed_value::IndexedValueError, substate::SubstateId};
use tari_ootle_common_types::{Epoch, InputDeclaration, declare_inputs};
use tari_template_lib_types::{ComponentAddress, crypto::RistrettoPublicKeyBytes};

use crate::{
    AuthorizationContext,
    BlobIndex,
    Blobs,
    Instruction,
    MainSigner,
    SchemaVersion,
    Signable,
    TransactionSignature,
    UnsealedTransactionV1,
    UnsignedTransactionV1,
    blobs::{Blob, BlobIndexOverflow},
    unsealed::UnsealedTransaction,
};

#[derive(Debug, Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum UnsignedTransaction {
    #[n(0)]
    V1(#[n(0)] UnsignedTransactionV1),
    #[n(1)]
    V2(#[n(0)] UnsignedTransactionV1),
}

impl UnsignedTransaction {
    /// A new [`SchemaVersion::CURRENT`] transaction.
    pub fn new<N: Into<u8>>(network: N, max_epoch: Epoch) -> Self {
        UnsignedTransactionV1::new_default(network, max_epoch).into()
    }

    pub const fn schema(&self) -> SchemaVersion {
        match self {
            Self::V1(_) => SchemaVersion::V1,
            Self::V2(_) => SchemaVersion::V2,
        }
    }

    pub fn body(&self) -> &UnsignedTransactionV1 {
        match self {
            Self::V1(tx) | Self::V2(tx) => tx,
        }
    }

    pub fn into_body(self) -> UnsignedTransactionV1 {
        match self {
            Self::V1(tx) | Self::V2(tx) => tx,
        }
    }

    pub fn body_mut(&mut self) -> &mut UnsignedTransactionV1 {
        match self {
            Self::V1(tx) | Self::V2(tx) => tx,
        }
    }

    /// Wraps `body` in the variant for `schema`.
    pub fn from_body(schema: SchemaVersion, body: UnsignedTransactionV1) -> Self {
        match schema {
            SchemaVersion::V1 => Self::V1(body),
            SchemaVersion::V2 => Self::V2(body),
        }
    }

    pub fn then<F: FnOnce(Self) -> Self>(self, f: F) -> Self {
        f(self)
    }

    pub fn schema_version(&self) -> u16 {
        self.schema().as_u16()
    }

    pub fn set_network<N: Into<u8>>(&mut self, network: N) -> &mut Self {
        match self {
            Self::V1(tx) | Self::V2(tx) => tx.set_network(network),
        };
        self
    }

    pub fn network(&self) -> u8 {
        match self {
            Self::V1(tx) | Self::V2(tx) => tx.network,
        }
    }

    pub fn is_dry_run(&self) -> bool {
        match self {
            Self::V1(tx) | Self::V2(tx) => tx.dry_run,
        }
    }

    pub(crate) fn set_dry_run(&mut self, dry_run: bool) -> &mut Self {
        match self {
            Self::V1(tx) | Self::V2(tx) => tx.set_dry_run(dry_run),
        };
        self
    }

    pub fn main_signer(&self) -> &MainSigner {
        match self {
            Self::V1(tx) | Self::V2(tx) => &tx.main_signer,
        }
    }

    /// Sets the transaction's main signer, which also decides whether the seal signature authorises the
    /// transaction.
    ///
    /// The field defaults to [`MainSigner::SealSigner`] and is part of the signing domain; sealing never changes
    /// it, so the value set here is the one every signature commits to and the engine enforces.
    pub fn set_main_signer(&mut self, main_signer: MainSigner) -> &mut Self {
        match self {
            Self::V1(tx) | Self::V2(tx) => tx.main_signer = main_signer,
        }
        self
    }

    pub fn fee_instructions(&self) -> &[Instruction] {
        match self {
            Self::V1(tx) | Self::V2(tx) => tx.fee_instructions(),
        }
    }

    pub(crate) fn fee_instructions_mut(&mut self) -> &mut Vec<Instruction> {
        match self {
            Self::V1(tx) | Self::V2(tx) => &mut tx.fee_instructions,
        }
    }

    pub fn instructions(&self) -> &[Instruction] {
        match self {
            Self::V1(tx) | Self::V2(tx) => tx.instructions(),
        }
    }

    pub(crate) fn instructions_mut(&mut self) -> &mut Vec<Instruction> {
        match self {
            Self::V1(tx) | Self::V2(tx) => &mut tx.instructions,
        }
    }

    pub fn into_instructions(self) -> Vec<Instruction> {
        match self {
            Self::V1(tx) | Self::V2(tx) => tx.instructions,
        }
    }

    pub fn inputs(&self) -> &IndexSet<InputDeclaration> {
        match self {
            Self::V1(tx) | Self::V2(tx) => tx.inputs(),
        }
    }

    pub fn add_input(&mut self, input: InputDeclaration) -> &mut Self {
        match self {
            Self::V1(tx) | Self::V2(tx) => {
                tx.add_input(input);
            },
        }
        self
    }

    pub fn min_epoch(&self) -> Option<Epoch> {
        match self {
            Self::V1(tx) | Self::V2(tx) => tx.min_epoch(),
        }
    }

    pub fn max_epoch(&self) -> Epoch {
        match self {
            Self::V1(tx) | Self::V2(tx) => tx.max_epoch(),
        }
    }

    pub fn as_referenced_components(&self) -> impl Iterator<Item = &ComponentAddress> + '_ {
        match self {
            Self::V1(tx) | Self::V2(tx) => tx.as_referenced_components(),
        }
    }

    /// Returns all substates addresses referenced by this transaction
    pub fn to_referenced_substates(&self) -> Result<HashSet<SubstateId>, IndexedValueError> {
        match self {
            Self::V1(tx) | Self::V2(tx) => tx.to_referenced_substates(),
        }
    }

    pub fn has_inputs_without_version(&self) -> bool {
        self.inputs().iter().any(|i| i.version().is_none())
    }

    pub fn set_min_epoch(&mut self, min_epoch: Option<Epoch>) -> &mut Self {
        match self {
            Self::V1(tx) | Self::V2(tx) => tx.min_epoch = min_epoch,
        }
        self
    }

    pub fn set_max_epoch(&mut self, max_epoch: Epoch) -> &mut Self {
        match self {
            Self::V1(tx) | Self::V2(tx) => tx.max_epoch = max_epoch,
        }
        self
    }

    pub fn nonce(&self) -> u64 {
        match self {
            Self::V1(tx) | Self::V2(tx) => tx.nonce,
        }
    }

    pub fn set_nonce(&mut self, nonce: u64) -> &mut Self {
        match self {
            Self::V1(tx) | Self::V2(tx) => tx.nonce = nonce,
        }
        self
    }

    pub fn with_inputs<I: IntoIterator<Item = InputDeclaration>>(mut self, inputs: I) -> Self {
        declare_inputs(self.inputs_mut(), inputs);
        self
    }

    /// Declares as a read each write-declared input for which `is_read` returns true, keeping the
    /// declarations and their order. A read declaration is left a read.
    pub fn narrow_to_reads<F: FnMut(&InputDeclaration) -> bool>(mut self, mut is_read: F) -> Self {
        let inputs = std::mem::take(self.inputs_mut());
        *self.inputs_mut() = inputs
            .into_iter()
            .map(|decl| {
                let narrow = decl.is_write() && is_read(&decl);
                if narrow { decl.with_is_write(false) } else { decl }
            })
            .collect();
        self
    }

    /// Signs the transaction with every key in `secrets`, each under `seal_signer` and the set of
    /// their public keys. The authorization list holds exactly those signatures, and the result is
    /// a [`SchemaVersion::CURRENT`] transaction whatever schema `self` had.
    pub fn authorize<'a, I>(self, seal_signer: &RistrettoPublicKeyBytes, secrets: I) -> UnsealedTransaction
    where I: IntoIterator<Item = &'a RistrettoSecretKey> {
        UnsealedTransactionV1::authorize(self.into_body(), seal_signer, secrets).into()
    }

    /// Attaches `signatures` as the authorization list of a [`SchemaVersion::CURRENT`] transaction,
    /// whatever schema `self` had. The list is ordered by public key keeping the first signature of
    /// a repeated key, and each signature must have been made over the set of all their keys.
    pub fn with_signatures(self, signatures: Vec<TransactionSignature>) -> UnsealedTransaction {
        UnsealedTransactionV1::with_canonical_signatures(self.into_body(), signatures).into()
    }

    pub fn with_dry_run(mut self, dry_run: bool) -> Self {
        match self {
            Self::V1(ref mut tx) | Self::V2(ref mut tx) => {
                tx.set_dry_run(dry_run);
            },
        }
        self
    }

    pub fn finish(self) -> UnsealedTransaction {
        self.with_signatures(vec![])
    }

    pub(crate) fn inputs_mut(&mut self) -> &mut IndexSet<InputDeclaration> {
        match self {
            Self::V1(tx) | Self::V2(tx) => &mut tx.inputs,
        }
    }

    pub fn blobs(&self) -> &Blobs {
        match self {
            Self::V1(tx) | Self::V2(tx) => tx.blobs(),
        }
    }

    pub(crate) fn blobs_mut(&mut self) -> &mut Blobs {
        match self {
            Self::V1(tx) | Self::V2(tx) => tx.blobs_mut(),
        }
    }

    /// Append a blob and return its assigned index. Errors if blob count would overflow
    /// `BlobIndex` capacity.
    pub fn add_blob(&mut self, blob: Blob) -> Result<BlobIndex, BlobIndexOverflow> {
        self.blobs_mut().push(blob)
    }

    /// Validate blob index references and blob coverage. See
    /// `UnsignedTransactionV1::validate_blob_references`.
    pub fn validate_blob_references(&self) -> Result<(), crate::v1::BlobValidationError> {
        match self {
            Self::V1(tx) | Self::V2(tx) => tx.validate_blob_references(),
        }
    }
}

/// Wraps the body as a [`SchemaVersion::CURRENT`] transaction.
impl From<UnsignedTransactionV1> for UnsignedTransaction {
    fn from(tx: UnsignedTransactionV1) -> Self {
        Self::V2(tx)
    }
}

impl Signable<AuthorizationContext<'_>> for UnsignedTransaction {
    type Signature = TransactionSignature;

    fn to_signing_message(&self, context: AuthorizationContext<'_>) -> crate::TransactionMessage {
        TransactionSignature::create_message(context, self.body())
    }
}
