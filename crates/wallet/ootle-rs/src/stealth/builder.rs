//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashMap;

use indexmap::IndexSet;
use ootle_byte_type::FromByteType;
use tari_crypto::ristretto::RistrettoSecretKey;
use tari_ootle_common_types::engine_types::{stealth::validate_transfer, substate::SubstateId};
use tari_ootle_wallet_crypto::balance_proof::{
    generate_stealth_balance_proof_signature,
    validate_balance_proof_signature,
};
use tari_template_lib_types::{
    stealth::{StealthInput, StealthInputsStatement, StealthTransferStatement},
    Amount,
    ResourceAddress,
    UtxoAddress,
};

use crate::{
    provider::{Provider, WalletProvider},
    stealth::{
        error::{InvalidStealthInputError, StealthProviderError},
        spec::Output,
        SignatureRequirements,
        StealthSignerRequirement,
    },
    wallet::{OotleWallet, WalletResult},
    Address,
};

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
    #[allow(clippy::too_many_lines)]
    pub async fn prepare(self) -> WalletResult<(StealthTransferStatement, SignatureRequirements)> {
        let substate_id_to_addr_map = self
            .spec
            .inputs_to_spend
            .iter()
            .map(|(addr, i)| {
                (
                    SubstateId::from(UtxoAddress::new(self.spec.resource_address, i.commitment.into())),
                    addr,
                )
            })
            .collect::<HashMap<_, _>>();

        let found_substates = self
            .provider
            .fetch_substates(substate_id_to_addr_map.keys().cloned())
            .await
            .map_err(|e| StealthProviderError::UnexpectedError {
                details: format!("Failed to fetch stealth input substates: {}", e),
            })?;
        if found_substates.len() != self.spec.inputs_to_spend.len() {
            return Err(StealthProviderError::UnexpectedError {
                details: "Some stealth inputs could not be found in the provider substates".to_string(),
            }
            .into());
        }

        let mut required_signers = IndexSet::with_capacity(found_substates.len());
        let mut seal_signer = None;
        let must_sign_with_account_key = self.spec.revealed_input_amount.is_positive();

        let mut agg_input_mask = RistrettoSecretKey::default();
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
            let Some(spender_addr) = substate_id_to_addr_map.get(&id).copied() else {
                tracing::warn!(
                    "The provider returned a substate that we did not request: {id}. We'll continue but that should \
                     never happen!"
                );
                continue;
            };
            if !must_sign_with_account_key && seal_signer.is_none() {
                seal_signer = Some(StealthSignerRequirement::new(spender_addr.clone(), public_nonce));
            } else {
                required_signers.insert(StealthSignerRequirement::new(spender_addr.clone(), public_nonce));
            }

            let commitment = address.id().into_commitment_bytes();

            let decrypted = self
                .provider
                .wallet()
                .decrypt_input_data(&commitment, input.output(), true)
                .await?;

            agg_input_mask = &agg_input_mask + decrypted.mask();
        }

        let total_output_amount = self.spec.total_output_amount();
        let total_revealed_input = self.spec.revealed_input_amount;

        let (outputs_statement, agg_output_mask) = self
            .provider
            .wallet()
            .generate_outputs_statement(self.spec.outputs, self.spec.revealed_output_amount)
            .await?;

        let inputs_statement = StealthInputsStatement {
            inputs: self.spec.inputs_to_spend.into_values().collect(),
            revealed_amount: total_revealed_input,
        };

        // If the transfer does not use any stealth inputs or outputs, no balance proof is required.
        let requires_balance_proof = !inputs_statement.inputs.is_empty() || !outputs_statement.outputs.is_empty();

        let balance_proof = requires_balance_proof.then(|| {
            generate_stealth_balance_proof_signature(
                &agg_input_mask,
                &agg_output_mask,
                &inputs_statement,
                &outputs_statement,
            )
        });

        if let Some(ref balance_proof) = &balance_proof {
            // Check that the provided inputs and outputs balance
            // We assume that the code has otherwise generated valid proofs, so the only reason this can fail
            // is if the input values and output values do not balance.
            if !validate_balance_proof_signature(balance_proof, &inputs_statement, &outputs_statement) {
                return Err(StealthProviderError::UnbalancedTransfer {
                    total_revealed_input,
                    output_amount: total_output_amount,
                }
                .into());
            }
        }

        let signatures = if must_sign_with_account_key {
            SignatureRequirements::new_must_sign_with_account_key(required_signers)
        } else {
            SignatureRequirements::new_opt_with_seal_signer(required_signers, seal_signer)
        };

        let transfer = StealthTransferStatement {
            inputs_statement,
            outputs_statement,
            balance_proof,
        };

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

    pub fn spend_stealth_input<I: Into<StealthInput>>(mut self, owner_address: Address, input: I) -> Self {
        let input = input.into();
        self.spec.inputs_to_spend.insert(owner_address, input);
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
    pub inputs_to_spend: HashMap<Address, StealthInput>,
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
