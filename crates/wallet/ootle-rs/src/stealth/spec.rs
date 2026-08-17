//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::num::NonZeroU64;

use indexmap::IndexSet;
use tari_crypto::ristretto::RistrettoPublicKey;
use tari_ootle_common_types::engine_types::crypto::OutputBody;
use tari_ootle_wallet_crypto::{memo::Memo, pay_to::PayTo};
use tari_template_lib_types::{
    Amount,
    EncryptedData,
    ResourceAddress,
    crypto::{PedersenCommitmentBytes, UtxoTag},
    stealth::{SpendCondition, StealthInput, TemplateFunction},
};

use crate::Address;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct StealthSignerRequirement {
    signer: Address,
    public_nonce: RistrettoPublicKey,
}

impl StealthSignerRequirement {
    pub fn new(signer: Address, public_nonce: RistrettoPublicKey) -> Self {
        Self { signer, public_nonce }
    }

    pub fn signer(&self) -> &Address {
        &self.signer
    }

    pub fn public_nonce(&self) -> &RistrettoPublicKey {
        &self.public_nonce
    }
}

/// The key that seals a stealth transaction.
///
/// Every authorization signature commits to the seal signer's public key, so which key seals must be settled — and
/// its public key known — before any authorization is produced.
// One instance exists per transaction, so the size gap to the unit variants does not matter.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SealSource {
    /// The wallet's default account key seals. Required whenever the transaction touches the account component itself,
    /// e.g. it funds the transfer from a revealed input bucket.
    AccountKey,
    /// A stealth input's one-time key seals. The seal signature is that input's spend authorization, which is why the
    /// same signer does not also appear in [`SignatureRequirements::authorizers`].
    StealthInput(StealthSignerRequirement),
    /// Nothing is spent and the account is not touched, so a fresh one-time key seals and is discarded. This keeps the
    /// wallet's account key off a transaction that has no need of it.
    Ephemeral,
}

impl SealSource {
    /// Resolves two seals down to the one that seals the transaction, along with the stealth signer
    /// displaced in the process — which must then authorize instead. See
    /// [`SignatureRequirements::merge`] for the ordering and why it holds.
    fn take_precedence_over(self, other: Self) -> (Self, Option<StealthSignerRequirement>) {
        match (self, other) {
            (Self::AccountKey, Self::StealthInput(displaced)) | (Self::StealthInput(displaced), Self::AccountKey) => {
                (Self::AccountKey, Some(displaced))
            },
            (Self::AccountKey, _) | (_, Self::AccountKey) => (Self::AccountKey, None),
            (Self::StealthInput(seal), Self::StealthInput(displaced)) => (Self::StealthInput(seal), Some(displaced)),
            (Self::StealthInput(seal), Self::Ephemeral) | (Self::Ephemeral, Self::StealthInput(seal)) => {
                (Self::StealthInput(seal), None)
            },
            (Self::Ephemeral, Self::Ephemeral) => (Self::Ephemeral, None),
        }
    }
}

/// Which key seals a stealth transfer transaction, and which stealth signers must additionally authorize it.
///
/// Every stealth input has to be authorized by its owner's one-time key. One of those signers seals the transaction —
/// its seal signature doubles as its own authorization — and the rest sign an authorization committing to the seal
/// signer's public key. When the account component itself is accessed (a revealed input bucket) the account key must
/// seal instead, and every stealth input signer authorizes. When neither applies the public keys on the transaction
/// are ephemeral to an outside observer.
#[derive(Debug, Clone)]
pub struct SignatureRequirements {
    seal: SealSource,
    authorizers: IndexSet<StealthSignerRequirement>,
}

impl SignatureRequirements {
    /// The account key seals and each of `authorizers` authorizes with its one-time stealth key. Use when the
    /// transaction accesses the account component, e.g. it spends a revealed input bucket.
    pub fn account_key_seal_with(authorizers: IndexSet<StealthSignerRequirement>) -> Self {
        Self {
            seal: SealSource::AccountKey,
            authorizers,
        }
    }

    /// The account key seals and no stealth signers are derived. Use this when every authorization is supplied
    /// externally — e.g. a script-path `AccessRule` leaf whose keys the wallet does not own (adaptor-signature
    /// co-signers) — so the seal signer, and hence the authorization message, is known without consulting a key
    /// provider.
    pub fn account_key_seal() -> Self {
        Self::account_key_seal_with(IndexSet::new())
    }

    /// The first of `signers` seals with its one-time stealth key and the rest authorize against it. With no signers
    /// there is nothing to authorize and an ephemeral key seals.
    pub fn stealth_seal(signers: IndexSet<StealthSignerRequirement>) -> Self {
        let mut signers = signers.into_iter();
        match signers.next() {
            Some(seal_signer) => Self::stealth_seal_with(seal_signer, signers.collect()),
            None => Self {
                seal: SealSource::Ephemeral,
                authorizers: IndexSet::new(),
            },
        }
    }

    /// `seal_signer` seals with its one-time stealth key and each of `authorizers` authorizes against it. Use when the
    /// sealing input is not the first one.
    pub fn stealth_seal_with(
        seal_signer: StealthSignerRequirement,
        authorizers: IndexSet<StealthSignerRequirement>,
    ) -> Self {
        Self {
            seal: SealSource::StealthInput(seal_signer),
            authorizers,
        }
    }

    pub fn seal(&self) -> &SealSource {
        &self.seal
    }

    /// Combines the requirements of two statements carried by one transaction.
    ///
    /// A transaction has a single seal, so one of the two gives way. An account key outranks a
    /// stealth input, since a transaction that touches the account component must be sealed by the
    /// account key whatever else it does, and either outranks an ephemeral key, which exists only
    /// for the case where nothing needs signing. A stealth signer whose seal gives way still has to
    /// authorize its own input, so it joins the authorizers.
    ///
    /// Splitting a transfer across the fee intent and the main intent is what makes this necessary:
    /// the fee intent may carry one statement, and any further statement belongs in the main intent
    /// where the fee just paid funds its verification.
    #[must_use]
    pub fn merge(self, other: Self) -> Self {
        let (seal, displaced) = self.seal.take_precedence_over(other.seal);
        let mut authorizers = self.authorizers;
        authorizers.extend(displaced);
        authorizers.extend(other.authorizers);
        // A sealing input's seal signature is its own authorization, so it must not also be asked
        // for one.
        if let SealSource::StealthInput(seal_signer) = &seal {
            authorizers.swap_remove(seal_signer);
        }
        Self { seal, authorizers }
    }

    pub fn into_parts(self) -> (SealSource, IndexSet<StealthSignerRequirement>) {
        (self.seal, self.authorizers)
    }

    /// The signers that must produce an authorization signature, each committing to the seal signer's public key.
    pub fn authorizers(&self) -> impl ExactSizeIterator<Item = &StealthSignerRequirement> {
        self.authorizers.iter()
    }
}

#[derive(Debug, Clone)]
pub struct Output {
    pub destination: Address,
    pub amount: NonZeroU64,
    pub resource_address: ResourceAddress,
    pub resource_view_key: Option<RistrettoPublicKey>,
    pub memo: Option<Memo>,
    pub pay_to: PayTo,
    pub utxo_tag: Option<UtxoTag>,
    pub minimum_value_promise: u64,
}

impl Output {
    pub fn new(destination: Address, resource_address: ResourceAddress, amount: NonZeroU64) -> Self {
        Self {
            destination,
            amount,
            resource_address,
            resource_view_key: None,
            memo: None,
            pay_to: PayTo::default(),
            utxo_tag: None,
            minimum_value_promise: 0,
        }
    }

    pub fn with_resource_view_key(mut self, resource_view_key: RistrettoPublicKey) -> Self {
        self.resource_view_key = Some(resource_view_key);
        self
    }

    pub fn with_memo(mut self, memo: Memo) -> Self {
        self.memo = Some(memo);
        self
    }

    /// Convenience method to create a text memo.
    ///
    /// # Panics
    /// Panics if the message is too long to fit in a memo.
    pub fn with_memo_message<T: Into<Box<str>>>(self, message: T) -> Self {
        self.with_memo(Memo::new_message(message).expect("Memo message too long"))
    }

    pub fn with_pay_to(mut self, pay_to: PayTo) -> Self {
        self.pay_to = pay_to;
        self
    }

    /// Gate this output's spend on a stateless WASM predicate (a single-leaf `TemplateFunction` condition tree). The
    /// value is still encrypted to `destination` so the recipient can discover and decrypt it; spending requires
    /// revealing and satisfying `template_function`.
    pub fn with_spend_script(self, template_function: TemplateFunction) -> Self {
        self.with_pay_to(PayTo::TemplateFunction(template_function))
    }

    /// Gate this output's spend on a condition tree (MAST) of alternative spend conditions. The output commits the
    /// Merkle root over `conditions`; a spender later reveals exactly ONE leaf plus an inclusion proof. Each leaf may
    /// be a native access rule, a WASM predicate ([`TemplateFunction`]), a native
    /// [`BuiltinPredicate`](tari_template_lib_types::stealth::BuiltinPredicate) (timelock, hashlock) or covenant — and
    /// a leaf is a conjunction (logical AND) of one or more such atoms.
    pub fn with_spend_conditions(self, conditions: Vec<SpendCondition>) -> Self {
        self.with_pay_to(PayTo::Conditions(conditions))
    }

    pub fn with_utxo_tag(mut self, utxo_tag: UtxoTag) -> Self {
        self.utxo_tag = Some(utxo_tag);
        self
    }
}

/// A stealth input resolved against the network and ready for statement construction.
///
/// Resolving an input — fetching its UTXO substate, rejecting frozen or burnt ones and recovering its
/// public nonce — needs network access but no keys, so it happens on the caller's side of
/// [`StealthStatementProvider`](crate::stealth::StealthStatementProvider). What remains is
/// key-dependent: recovering the input's mask from `output`.
#[derive(Debug, Clone)]
pub struct ResolvedStealthInput {
    /// The input as it appears in the statement: the commitment being spent plus the spend witness
    /// selecting its authorisation path.
    pub input: StealthInput,
    /// The on-chain output body the input's mask is recovered from.
    pub output: OutputBody,
}

impl ResolvedStealthInput {
    pub fn new(input: StealthInput, output: OutputBody) -> Self {
        Self { input, output }
    }

    pub fn commitment(&self) -> &PedersenCommitmentBytes {
        &self.input.commitment
    }
}

/// A stealth transfer whose inputs are resolved, ready to be turned into a
/// [`StealthTransferStatement`](tari_template_lib_types::stealth::StealthTransferStatement) by a
/// [`StealthStatementProvider`](crate::stealth::StealthStatementProvider).
#[derive(Debug, Clone)]
pub struct ResolvedStealthTransferSpec {
    pub inputs: Vec<ResolvedStealthInput>,
    /// Revealed amount the transfer consumes from a bucket.
    pub revealed_input_amount: Amount,
    pub outputs: Vec<Output>,
    /// Revealed amount the transfer pays out, e.g. to cover a fee.
    pub revealed_output_amount: Amount,
}

impl ResolvedStealthTransferSpec {
    pub fn total_output_amount(&self) -> Amount {
        let stealth_output_total: Amount = self.outputs.iter().map(|o| Amount::from(o.amount.get())).sum();
        stealth_output_total + self.revealed_output_amount
    }

    /// Whether the transfer needs a balance proof. A transfer with no stealth inputs and no stealth
    /// outputs moves only revealed value and has nothing to balance.
    pub fn requires_balance_proof(&self) -> bool {
        !self.inputs.is_empty() || !self.outputs.is_empty()
    }
}

/// The claimed-funds side of an L1 burn claim, ready to be turned into a statement by
/// [`BurnClaimKeyProvider::create_burn_claim_statement`](crate::stealth::BurnClaimKeyProvider::create_burn_claim_statement).
#[derive(Debug, Clone)]
pub struct BurnClaimStatementSpec {
    /// The burn UTXO's commitment, taken from the L1 burn proof.
    pub commitment: PedersenCommitmentBytes,
    /// The L1 output's encrypted data, which the burn UTXO's mask is recovered from.
    pub encrypted_data: EncryptedData,
    /// `R`, the public nonce the L1 UTXO was burnt with.
    pub sender_offset_public_key: RistrettoPublicKey,
    /// The stealth output the claimed funds are paid into.
    pub output: Output,
    /// Revealed amount reserved to pay the claim transaction's fee.
    pub revealed_output_amount: Amount,
}

#[cfg(test)]
mod tests {
    use ootle_byte_type::ToByteType;
    use tari_crypto::{
        keys::{PublicKey, SecretKey},
        ristretto::RistrettoSecretKey,
    };

    use super::*;
    use crate::Network;

    fn signer_from_seed(seed: u8) -> StealthSignerRequirement {
        let secret = RistrettoSecretKey::from_uniform_bytes(&[seed; 64]).unwrap();
        let pk = RistrettoPublicKey::from_secret_key(&secret);
        let addr = Address::new(Network::LocalNet, pk.to_byte_type(), pk.to_byte_type());
        StealthSignerRequirement::new(addr, pk)
    }

    fn signers<const N: usize>(seeds: [u8; N]) -> IndexSet<StealthSignerRequirement> {
        seeds.into_iter().map(signer_from_seed).collect()
    }

    mod signature_requirement_invariants {
        use super::*;

        /// The account key seals and every stealth signer authorizes: none of them is promoted to seal signer.
        #[test]
        fn account_key_seal_authorizes_every_signer() {
            let spec = SignatureRequirements::account_key_seal_with(signers([1, 2]));

            assert_eq!(spec.seal(), &SealSource::AccountKey);
            assert_eq!(spec.authorizers().collect::<Vec<_>>(), vec![
                &signer_from_seed(1),
                &signer_from_seed(2)
            ]);
        }

        /// With no stealth signers the account key is the only signature on the transaction.
        #[test]
        fn account_key_seal_without_signers_has_no_authorizers() {
            let spec = SignatureRequirements::account_key_seal();

            assert_eq!(spec.seal(), &SealSource::AccountKey);
            assert_eq!(spec.authorizers().len(), 0);
        }

        /// The first stealth signer is promoted to seal signer; the rest authorize against it.
        #[test]
        fn stealth_seal_promotes_the_first_signer() {
            let spec = SignatureRequirements::stealth_seal(signers([1, 2, 3]));

            assert_eq!(spec.seal(), &SealSource::StealthInput(signer_from_seed(1)));
            assert_eq!(spec.authorizers().collect::<Vec<_>>(), vec![
                &signer_from_seed(2),
                &signer_from_seed(3)
            ]);
        }

        /// A single stealth input seals and needs no authorization: the seal signature is its own.
        #[test]
        fn a_single_stealth_signer_seals_and_authorizes_nothing() {
            let spec = SignatureRequirements::stealth_seal(signers([1]));

            assert_eq!(spec.seal(), &SealSource::StealthInput(signer_from_seed(1)));
            assert_eq!(spec.authorizers().len(), 0);
        }

        /// An explicitly chosen seal signer is not one of the authorizers.
        #[test]
        fn stealth_seal_with_an_explicit_seal_signer() {
            let spec = SignatureRequirements::stealth_seal_with(signer_from_seed(2), signers([1, 3]));

            assert_eq!(spec.seal(), &SealSource::StealthInput(signer_from_seed(2)));
            assert_eq!(spec.authorizers().collect::<Vec<_>>(), vec![
                &signer_from_seed(1),
                &signer_from_seed(3)
            ]);
        }

        /// Nothing to spend and no account access: an ephemeral key seals.
        #[test]
        fn no_signers_seals_ephemerally() {
            let spec = SignatureRequirements::stealth_seal(IndexSet::new());

            assert_eq!(spec.seal(), &SealSource::Ephemeral);
            assert_eq!(spec.authorizers().len(), 0);
        }
    }

    /// Merging is what lets one transaction carry a fee-sourcing statement and a second, larger one
    /// whose verification the fee pays for. The transaction still has a single seal, so the cases
    /// below fix which of the two survives and what becomes of the signer it displaces.
    mod merging_two_statements {
        use super::*;

        /// The displaced statement's seal signer still owns an input, so it has to authorize.
        #[test]
        fn a_displaced_stealth_seal_becomes_an_authorizer() {
            let fee = SignatureRequirements::stealth_seal(signers([1]));
            let transfer = SignatureRequirements::stealth_seal(signers([2]));

            let merged = fee.merge(transfer);

            assert_eq!(merged.seal(), &SealSource::StealthInput(signer_from_seed(1)));
            assert_eq!(merged.authorizers().cloned().collect::<Vec<_>>(), vec![
                signer_from_seed(2)
            ]);
        }

        /// Authorizers from both statements are carried over, not just the displaced seal signer.
        #[test]
        fn authorizers_from_both_statements_are_kept() {
            let fee = SignatureRequirements::stealth_seal(signers([1, 2]));
            let transfer = SignatureRequirements::stealth_seal(signers([3, 4]));

            let merged = fee.merge(transfer);

            assert_eq!(merged.seal(), &SealSource::StealthInput(signer_from_seed(1)));
            assert_eq!(merged.authorizers().cloned().collect::<Vec<_>>(), vec![
                signer_from_seed(2),
                signer_from_seed(3),
                signer_from_seed(4),
            ]);
        }

        /// A transaction touching the account component must be sealed by the account key, whichever
        /// statement needed it and whatever the other one asked for.
        #[test]
        fn an_account_key_seal_outranks_a_stealth_one() {
            let fee = SignatureRequirements::stealth_seal(signers([1]));
            let transfer = SignatureRequirements::account_key_seal_with(signers([2]));

            let merged = fee.clone().merge(transfer.clone());
            assert_eq!(merged.seal(), &SealSource::AccountKey);
            // Nothing seals on its behalf now, so signer 1 authorizes too.
            assert_eq!(merged.authorizers().cloned().collect::<Vec<_>>(), vec![
                signer_from_seed(1),
                signer_from_seed(2),
            ]);

            // The same holds whichever way round the two are merged.
            assert_eq!(transfer.merge(fee).seal(), &SealSource::AccountKey);
        }

        /// An ephemeral seal exists only where nothing needs signing, so it yields to anything real.
        #[test]
        fn an_ephemeral_seal_yields() {
            let ephemeral = SignatureRequirements::stealth_seal(IndexSet::new());
            let stealth = SignatureRequirements::stealth_seal(signers([1]));

            assert_eq!(
                ephemeral.clone().merge(stealth.clone()).seal(),
                &SealSource::StealthInput(signer_from_seed(1))
            );
            assert_eq!(
                stealth.merge(ephemeral.clone()).seal(),
                &SealSource::StealthInput(signer_from_seed(1))
            );
            assert_eq!(ephemeral.clone().merge(ephemeral).seal(), &SealSource::Ephemeral);
        }

        /// The sealing signature doubles as that input's authorization, so the seal signer must not
        /// also be asked for one.
        #[test]
        fn the_sealing_signer_is_never_also_an_authorizer() {
            let fee = SignatureRequirements::stealth_seal(signers([1]));
            let transfer = SignatureRequirements::account_key_seal_with(signers([1]));

            // Account key seals, so signer 1 appears only once, as an authorizer.
            let merged = fee.clone().merge(transfer);
            assert_eq!(merged.authorizers().cloned().collect::<Vec<_>>(), vec![
                signer_from_seed(1)
            ]);

            // Signer 1 seals, so it is not also listed.
            let merged = fee.merge(SignatureRequirements::stealth_seal(signers([1])));
            assert_eq!(merged.seal(), &SealSource::StealthInput(signer_from_seed(1)));
            assert_eq!(merged.authorizers().len(), 0);
        }
    }
}
