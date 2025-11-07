//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_bor::{Deserialize, Serialize};
use tari_engine_types::{crypto::MAX_LAZY_BP_AGG_FACTORS, FromByteType};
use tari_ootle_address::OotleAddress;
use tari_ootle_common_types::Network;
use tari_ootle_wallet_crypto::memo::Memo;
use tari_template_lib::{
    models::{NonFungibleAddress, ResourceAddress},
    prelude::Amount,
};

use crate::apis::{
    confidential_transfer::UtxoInputSelection,
    stealth_transfer::{StealthOutputToCreate, StealthTransferApiError},
};

#[derive(Debug)]
pub struct StealthTransferParams {
    /// Strategy for input selection
    pub fee_input_selection: UtxoInputSelection,
    /// Strategy for input selection
    pub input_selection: UtxoInputSelection,
    pub outputs: Vec<TransferOutput>,
    pub badge_usage: BadgeUsage,
    /// Address of the resource to transfer
    pub resource_address: ResourceAddress,
    /// Fee to lock for the transaction
    pub max_fee: u64,
    /// Run as a dry run, no funds will be transferred if true
    pub is_dry_run: bool,
}

impl StealthTransferParams {
    pub fn validate(&self, network: Network) -> Result<(), StealthTransferApiError> {
        if self.outputs.is_empty() {
            return Err(StealthTransferApiError::InvalidParameter {
                param: "outputs",
                reason: "At least one output must be specified".to_string(),
            });
        }

        let blinded_count = self.outputs.iter().filter(|o| o.blinded_amount > 0).count();
        if blinded_count > MAX_LAZY_BP_AGG_FACTORS {
            return Err(StealthTransferApiError::InvalidParameter {
                param: "outputs",
                reason: format!(
                    "Number of outputs ({}) exceeds maximum allowed ({})",
                    blinded_count, MAX_LAZY_BP_AGG_FACTORS
                ),
            });
        }

        for output in &self.outputs {
            if output.revealed_amount.is_negative() {
                return Err(StealthTransferApiError::InvalidParameter {
                    param: "revealed_output_amount",
                    reason: "Revealed output amount must be non-negative".to_string(),
                });
            }

            if output.blinded_amount == 0 && output.revealed_amount.is_zero() {
                return Err(StealthTransferApiError::InvalidParameter {
                    param: "blinded_output_amount and revealed_output_amount",
                    reason: "At least one of the amounts must be greater than zero".to_string(),
                });
            }

            if output.address.network() != network {
                return Err(StealthTransferApiError::InvalidParameter {
                    param: "destination_address",
                    reason: format!(
                        "Destination address network ({}) does not match wallet network ({})",
                        output.address.network(),
                        network
                    ),
                });
            }

            output
                .address
                .validate()
                .map_err(|e| StealthTransferApiError::InvalidParameter {
                    param: "destination_address",
                    reason: format!("Invalid destination address: {}", e),
                })?;
        }

        Ok(())
    }

    pub fn total_output_amount(&self) -> Amount {
        self.outputs.iter().map(|o| o.total_output_amount()).sum()
    }

    pub fn total_revealed_output_amount(&self) -> Amount {
        self.outputs.iter().map(|o| o.revealed_amount).sum()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-daemon-client/"))]
pub struct TransferOutput {
    /// Destination address used to derive the UTXO encryption keys, owner signature and the account in which to
    /// deposit revealed funds
    pub address: OotleAddress,
    /// Amount to spend to a revealed output
    pub revealed_amount: Amount,
    /// Amount to spend to a blinded output
    pub blinded_amount: u64,
    /// Optional memo to include a memo in the output. This memo is encrypted and can only be read by the recipient.
    pub memo: Option<Memo>,
}

impl TransferOutput {
    pub fn total_output_amount(&self) -> Amount {
        self.revealed_amount + Amount::from(self.blinded_amount)
    }
}

impl<'a> TryFrom<&'a TransferOutput> for StealthOutputToCreate<'a> {
    type Error = StealthTransferApiError;

    fn try_from(value: &'a TransferOutput) -> Result<Self, Self::Error> {
        Ok(Self {
            owner_address: value.address.try_from_byte_type().map_err(|e| {
                StealthTransferApiError::InvalidParameter {
                    param: "destination_address",
                    reason: format!("Invalid destination address: {}", e),
                }
            })?,
            amount: value.blinded_amount,
            memo: value.memo.as_ref(),
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-daemon-client/"))]
pub enum BadgeUsage {
    /// Do not use a badge
    #[default]
    None,
    /// Use a resource as a badge
    Resource(ResourceAddress),
    /// Use a specific NFT as a badge
    NonFungible(NonFungibleAddress),
    /// Use a specified amount of resource as a badge
    AmountOfResource { resource: ResourceAddress, amount: Amount },
}

impl BadgeUsage {
    pub fn resource_address(&self) -> Option<&ResourceAddress> {
        match self {
            BadgeUsage::None => None,
            BadgeUsage::Resource(addr) => Some(addr),
            BadgeUsage::NonFungible(nft_addr) => Some(nft_addr.resource_address()),
            BadgeUsage::AmountOfResource { resource, .. } => Some(resource),
        }
    }

    pub fn is_none(&self) -> bool {
        matches!(self, BadgeUsage::None)
    }
}
