//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use indexmap::{IndexMap, IndexSet};
use ootle_byte_type::FromByteType;
use tari_ootle_common_types::engine_types::{stealth::validate_transfer, substate::SubstateId};
use tari_template_lib_types::{
    Amount,
    ResourceAddress,
    UtxoAddress,
    stealth::{StealthInput, StealthTransferStatement},
};

use crate::{
    Address,
    provider::{Provider, WalletProvider},
    stealth::{
        ResolvedStealthInput,
        ResolvedStealthTransferSpec,
        SignatureRequirements,
        StealthSignerRequirement,
        error::{InvalidStealthInputError, StealthProviderError},
        spec::Output,
    },
    wallet::{OotleWallet, WalletResult},
};

/// Builder for constructing confidential stealth transfers.
///
/// Supports revealed and stealth inputs, stealth outputs with optional encrypted memos,
/// change handling, and spending proof generation.
///
/// ```rust,ignore
/// let (statement, sig_reqs) = StealthTransfer::new(TARI_TOKEN, &provider)
///     .spend_revealed_input(commitment, mask, value)
///     .to_stealth_output(&recipient, 500_000u64, None)
///     .prepare()
///     .await?;
/// ```
pub struct StealthTransfer<'a, P> {
    provider: &'a P,
    spec: StealthTransferSpec,
}

impl<'a, P: Provider> StealthTransfer<'a, P> {
    pub fn new(resource_address: ResourceAddress, provider: &'a P) -> Self {
        Self {
            provider,
            spec: StealthTransferSpec::new(resource_address),
        }
    }
}

impl<'a, P: WalletProvider<Wallet = OotleWallet>> StealthTransfer<'a, P> {
    /// Build the stealth transfer statement without constructing the transaction
    pub async fn prepare(mut self) -> WalletResult<(StealthTransferStatement, SignatureRequirements)> {
        let total_output_amount = self.spec.total_output_amount();
        let total_revealed_input = self.spec.revealed_input_amount;

        let (resolved_inputs, signatures) = self.resolve_inputs().await?;

        let spec = ResolvedStealthTransferSpec {
            inputs: resolved_inputs,
            revealed_input_amount: total_revealed_input,
            outputs: self.spec.outputs,
            revealed_output_amount: self.spec.revealed_output_amount,
        };

        let transfer = self.provider.wallet().create_transfer_statement(spec).await?;

        if let Err(err) = validate_transfer(&transfer, None) {
            tracing::warn!("The constructed stealth transfer is unbalanced: {}", err);
            return Err(StealthProviderError::UnbalancedTransfer {
                total_revealed_input,
                output_amount: total_output_amount,
            }
            .into());
        }

        Ok((transfer, signatures))
    }

    /// Fetch each input's UTXO substate and pair it with the output body its mask is recovered from,
    /// deriving the transfer's signature requirements along the way.
    ///
    /// This is the network-dependent, key-independent half of [`prepare`](Self::prepare): everything
    /// here is public material, so it stays on this side of the
    /// [`StealthStatementProvider`](crate::stealth::StealthStatementProvider) boundary.
    async fn resolve_inputs(&mut self) -> WalletResult<(Vec<ResolvedStealthInput>, SignatureRequirements)> {
        // Keyed by the UTXO each input spends, so several inputs owned by one address stay distinct, and iterated in
        // the order the caller added them. A commitment may appear only once: the same UTXO cannot be spent twice, and
        // two entries naming it would otherwise silently collapse into one.
        let mut inputs_by_utxo = IndexMap::with_capacity(self.spec.inputs_to_spend.len());
        for (spender_addr, input) in self.spec.inputs_to_spend.drain(..) {
            let id = SubstateId::from(UtxoAddress::new(self.spec.resource_address, input.commitment.into()));
            if inputs_by_utxo.contains_key(&id) {
                return Err(StealthProviderError::UnexpectedError {
                    details: format!("The stealth input {id} was added to this transfer more than once"),
                }
                .into());
            }
            inputs_by_utxo.insert(id, (spender_addr, input));
        }

        let mut found_substates = self
            .provider
            .fetch_substates(inputs_by_utxo.keys().cloned())
            .await
            .map_err(|e| StealthProviderError::UnexpectedError {
                details: format!("Failed to fetch stealth input substates: {}", e),
            })?;

        let mut required_signers = IndexSet::with_capacity(inputs_by_utxo.len());
        // Accessing the account component to take the revealed input bucket requires the account key's badge, so it
        // must seal; otherwise the inputs' own one-time keys are all the transaction needs.
        let must_sign_with_account_key = self.spec.revealed_input_amount.is_positive();
        let mut resolved_inputs = Vec::with_capacity(inputs_by_utxo.len());

        // Driven by the caller's inputs rather than the fetched substates, which arrive in a `HashMap`: the first
        // input is promoted to seal signer, so iterating in a nondeterministic order would pick a different seal
        // signer, and order the statement's inputs differently, from one run to the next.
        for (id, (spender_addr, to_spend)) in inputs_by_utxo {
            // TODO: work on the error types
            let Some(address) = id.as_utxo_address() else {
                return Err(StealthProviderError::UnexpectedError {
                    details: format!("Expected UTXO address substate id, got: {}", id),
                }
                .into());
            };
            let Some(substate) = found_substates.remove(&id) else {
                return Err(StealthProviderError::UnexpectedError {
                    details: format!("The stealth input {id} could not be found in the provider substates"),
                }
                .into());
            };
            let Some(utxo) = substate.into_substate_value().into_utxo() else {
                return Err(StealthProviderError::UnexpectedError {
                    details: format!("Expected UTXO output substate but got another substate for {id}",),
                }
                .into());
            };

            if utxo.is_frozen {
                return Err(
                    StealthProviderError::InvalidInput(InvalidStealthInputError::UtxoIsFrozen { address }).into(),
                );
            }

            let input = utxo.output.ok_or_else(|| {
                StealthProviderError::InvalidInput(InvalidStealthInputError::UtxoIsBurnt {
                    address: address.clone(),
                })
            })?;

            let Ok(public_nonce) = input.output.public_nonce.try_from_byte_type() else {
                return Err(StealthProviderError::UnexpectedError {
                    details: format!(
                        "Failed to convert public nonce to RistrettoPublicKey for stealth input at address {}",
                        address
                    ),
                }
                .into());
            };
            required_signers.insert(StealthSignerRequirement::new(spender_addr, public_nonce));

            resolved_inputs.push(ResolvedStealthInput::new(to_spend, input.output().clone()));
        }

        let signatures = if must_sign_with_account_key {
            SignatureRequirements::account_key_seal_with(required_signers)
        } else {
            SignatureRequirements::stealth_seal(required_signers)
        };

        Ok((resolved_inputs, signatures))
    }

    /// When the stealth transfer is executed, it will expect some revealed amount as input from a bucket.
    /// How this bucket is created depends entirely on logic of the contract/transaction.
    /// If there is no revealed input amount provided, the transfer will fail.
    pub fn spend_revealed_input<A: Into<Amount>>(mut self, amount: A) -> Self {
        let amount: Amount = amount.into();
        if !amount.is_positive() {
            panic!("Transfer amount must be positive");
        }
        self.spec.revealed_input_amount += amount;
        self
    }

    /// Spend a stealth input owned by `owner_address`. Call repeatedly to spend several inputs, including several
    /// owned by the same address.
    pub fn spend_stealth_input<I: Into<StealthInput>>(mut self, owner_address: Address, input: I) -> Self {
        self.spec.inputs_to_spend.push((owner_address, input.into()));
        self
    }

    /// Add a stealth output to the transfer
    ///
    /// # Panics
    ///
    /// Panics if the output amount is zero
    pub fn to_stealth_output(mut self, output: Output) -> Self {
        self.spec.outputs.push(output);
        self
    }

    pub fn to_revealed_output<A: Into<Amount>>(mut self, amount: A) -> Self {
        let amount = amount.into();
        if !amount.is_positive() {
            panic!("Transfer amount must be positive");
        }
        self.spec.revealed_output_amount += amount;
        self
    }
}

#[derive(Debug, Clone)]
pub struct StealthTransferSpec {
    pub resource_address: ResourceAddress,
    pub revealed_input_amount: Amount,
    pub inputs_to_spend: Vec<(Address, StealthInput)>,
    pub outputs: Vec<Output>,
    pub revealed_output_amount: Amount,
}

impl StealthTransferSpec {
    pub fn new(resource_address: ResourceAddress) -> Self {
        Self {
            resource_address,
            revealed_input_amount: Amount::zero(),
            inputs_to_spend: Default::default(),
            outputs: Default::default(),
            revealed_output_amount: Amount::zero(),
        }
    }

    pub fn total_output_amount(&self) -> Amount {
        let stealth_output_total: Amount = self.outputs.iter().map(|o| Amount::from(o.amount.get())).sum();
        stealth_output_total + self.revealed_output_amount
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, HashSet},
        num::NonZeroU64,
        sync::Weak,
    };

    use tari_ootle_common_types::engine_types::{Utxo, UtxoOutput, crypto::OutputBody, substate::Substate};
    use tari_ootle_transaction::UnsignedTransaction;
    use tari_template_lib_types::{
        constants::TARI_TOKEN,
        crypto::UtxoTag,
        stealth::{SpendAuthorization, StealthUnspentOutput},
    };

    use super::*;
    use crate::{
        Network,
        key_provider::PrivateKeyProvider,
        provider::{ProviderResult, WantInput},
        stealth::{StealthOutputStatementFactory, spec::SealSource},
        transaction::TransactionSigner,
    };

    /// A provider that serves a fixed set of substates and nothing else. Only [`Provider::fetch_substates`] and
    /// [`WalletProvider::wallet`] are reached by input resolution.
    struct FixedSubstateProvider {
        wallet: OotleWallet,
        address: Address,
        substates: HashMap<SubstateId, Substate>,
    }

    impl Provider for FixedSubstateProvider {
        type Client = ();

        fn network(&self) -> Network {
            Network::LocalNet
        }

        fn weak_client(&self) -> Weak<Self::Client> {
            Weak::new()
        }

        fn default_signer_address(&self) -> &Address {
            &self.address
        }

        async fn resolve_input_want_list(
            &self,
            transaction: UnsignedTransaction,
            _want_list: &HashSet<WantInput>,
        ) -> ProviderResult<UnsignedTransaction> {
            Ok(transaction)
        }

        async fn fetch_substates<I: IntoIterator<Item = SubstateId> + Send>(
            &self,
            substate_ids: I,
        ) -> ProviderResult<HashMap<SubstateId, Substate>> {
            Ok(substate_ids
                .into_iter()
                .filter_map(|id| self.substates.get(&id).map(|s| (id, s.clone())))
                .collect())
        }
    }

    impl WalletProvider for FixedSubstateProvider {
        type Wallet = OotleWallet;

        fn wallet(&self) -> &Self::Wallet {
            &self.wallet
        }

        fn wallet_mut(&mut self) -> &mut Self::Wallet {
            &mut self.wallet
        }
    }

    /// Mint `count` stealth outputs owned by a fresh key provider and serve them as spendable UTXO substates.
    async fn provider_owning(count: usize) -> (FixedSubstateProvider, Address, Vec<StealthUnspentOutput>) {
        let key_provider = PrivateKeyProvider::random(Network::LocalNet);
        let address = key_provider.address().clone();

        let specs = (0..count)
            .map(|i| {
                Output::new(
                    address.clone(),
                    TARI_TOKEN,
                    NonZeroU64::new(1_000_000 + i as u64).expect("test value is non-zero"),
                )
            })
            .collect();
        let (statement, _mask) = key_provider
            .generate_outputs_statement(specs, Amount::zero())
            .await
            .expect("minting stealth outputs must succeed");

        let substates = statement
            .outputs
            .iter()
            .map(|minted| {
                let id = SubstateId::from(UtxoAddress::new(TARI_TOKEN, minted.output.commitment.into()));
                let utxo = Utxo::new(UtxoOutput {
                    output: OutputBody {
                        public_nonce: minted.output.sender_public_nonce,
                        encrypted_data: minted.output.encrypted_data.clone(),
                        minimum_value_promise: minted.output.minimum_value_promise,
                        viewable_balance: None,
                    },
                    auth: SpendAuthorization::Key(*address.account_public_key()),
                    tag: UtxoTag::new(0),
                });
                (id, Substate::new(0, utxo))
            })
            .collect();

        let provider = FixedSubstateProvider {
            wallet: OotleWallet::from(key_provider),
            address: address.clone(),
            substates,
        };
        (provider, address, statement.outputs)
    }

    /// The seal signer and the statement's input order follow the order inputs were added, not the hash order the
    /// provider happens to return its substates in.
    #[tokio::test]
    async fn input_resolution_follows_the_order_inputs_were_added() {
        let (provider, address, minted) = provider_owning(4).await;
        let commitments: Vec<_> = minted.iter().map(|o| o.output.commitment).collect();

        // Resolving the same inputs repeatedly must agree; a HashMap-ordered resolution would drift across runs.
        let mut seen = None;
        for _ in 0..8 {
            let mut transfer = StealthTransfer::new(TARI_TOKEN, &provider);
            for commitment in &commitments {
                transfer = transfer.spend_stealth_input(address.clone(), *commitment);
            }

            let (resolved, requirements) = transfer.resolve_inputs().await.expect("inputs are owned and unspent");

            let order: Vec<_> = resolved.iter().map(|i| *i.commitment()).collect();
            assert_eq!(order, commitments, "inputs must resolve in the order they were added");

            let SealSource::StealthInput(seal_signer) = requirements.seal() else {
                panic!("stealth inputs with no revealed input must seal with a stealth key");
            };
            let nonce = seal_signer.public_nonce().clone();
            assert_eq!(
                seen.get_or_insert_with(|| nonce.clone()),
                &nonce,
                "the seal signer must not vary across runs"
            );
        }
    }

    /// Spending the same UTXO twice is rejected rather than silently collapsing to a single input.
    #[tokio::test]
    async fn the_same_input_cannot_be_spent_twice() {
        let (provider, address, minted) = provider_owning(1).await;
        let commitment = minted[0].output.commitment;

        let err = StealthTransfer::new(TARI_TOKEN, &provider)
            .spend_stealth_input(address.clone(), commitment)
            .spend_stealth_input(address, commitment)
            .resolve_inputs()
            .await
            .expect_err("the same UTXO cannot be spent twice");

        assert!(err.to_string().contains("more than once"), "unexpected error: {err}");
    }
}
