//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashMap;

use indexmap::IndexSet;
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
        // Keyed by the UTXO each input spends, so that several inputs owned by the same address stay distinct.
        let mut inputs_by_utxo = self
            .spec
            .inputs_to_spend
            .drain(..)
            .map(|(addr, i)| {
                (
                    SubstateId::from(UtxoAddress::new(self.spec.resource_address, i.commitment.into())),
                    (addr, i),
                )
            })
            .collect::<HashMap<_, _>>();

        let found_substates = self
            .provider
            .fetch_substates(inputs_by_utxo.keys().cloned())
            .await
            .map_err(|e| StealthProviderError::UnexpectedError {
                details: format!("Failed to fetch stealth input substates: {}", e),
            })?;
        if found_substates.len() != inputs_by_utxo.len() {
            return Err(StealthProviderError::UnexpectedError {
                details: "Some stealth inputs could not be found in the provider substates".to_string(),
            }
            .into());
        }

        let mut required_signers = IndexSet::with_capacity(found_substates.len());
        // Accessing the account component to take the revealed input bucket requires the account key's badge, so it
        // must seal; otherwise the inputs' own one-time keys are all the transaction needs.
        let must_sign_with_account_key = self.spec.revealed_input_amount.is_positive();
        let mut resolved_inputs = Vec::with_capacity(found_substates.len());

        for (id, substate) in found_substates {
            // TODO: work on the error types
            let Some(address) = id.as_utxo_address() else {
                return Err(StealthProviderError::UnexpectedError {
                    details: format!("Expected UTXO address substate id, got: {}", id),
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
            let Some((spender_addr, to_spend)) = inputs_by_utxo.remove(&id) else {
                tracing::warn!(
                    "The provider returned a substate that we did not request: {id}. We'll continue but that should \
                     never happen!"
                );
                continue;
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
