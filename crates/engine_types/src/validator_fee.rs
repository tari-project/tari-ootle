//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use tari_template_lib::{
    auth::{OwnerRule, Ownership},
    types::{constants::XTR, crypto::RistrettoPublicKeyBytes, Amount, ValidatorFeePoolAddress},
};

use crate::resource_container::{ResourceContainer, ResourceError};

#[derive(Debug, Clone, Serialize, Deserialize, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ValidatorFeePool {
    #[cfg_attr(feature = "ts", ts(type = "ArrayBuffer"))]
    pub claim_public_key: RistrettoPublicKeyBytes,
    pub amount: u64,
}

impl ValidatorFeePool {
    pub fn new(claim_public_key: RistrettoPublicKeyBytes, amount: u64) -> Self {
        Self {
            claim_public_key,
            amount,
        }
    }

    pub fn as_ownership(&self) -> Ownership<'_> {
        Ownership {
            owner_key: Some(&self.claim_public_key),
            owner_rule: Cow::Owned(OwnerRule::OwnedBySigner),
        }
    }

    /// Withdraws the given amount from the pool. If the amount is greater than the current balance, the function will
    /// return false and the balance will remain unchanged.
    /// NB: Do not use this function in the engine. This is used at the consensus level to update fee substates in
    /// place.
    #[must_use]
    pub fn withdraw_direct(&mut self, amount: u64) -> bool {
        match self.amount.checked_sub(amount) {
            Some(new_amount) => {
                self.amount = new_amount;
                true
            },
            None => false,
        }
    }

    /// Deposits the given amount into the pool. Will return false and
    /// the balance will remain unchanged if the deposit overflows u64.
    /// NB: Do not use this function in the engine. This is used at the consensus level to update fee substates in
    /// place.
    #[must_use]
    pub fn deposit_direct(&mut self, amount: u64) -> bool {
        match self.amount.checked_add(amount) {
            Some(new_amount) => {
                self.amount = new_amount;
                true
            },
            None => false,
        }
    }

    pub fn amount(&self) -> u64 {
        self.amount
    }

    pub fn claim_public_key(&self) -> &RistrettoPublicKeyBytes {
        &self.claim_public_key
    }

    /// Withdraws all the funds from the pool and returns them in a ResourceContainer.
    /// If the pool has insufficient funds, an error is returned.
    /// This function is used in the engine to withdraw the funds from the pool and create a Bucket.
    pub fn withdraw_all(&mut self) -> Result<ResourceContainer, ResourceError> {
        if self.amount == 0 {
            return Err(ResourceError::InsufficientBalance {
                details: "ValidatorFeePool has insufficient balance. Current balance is 0".to_string(),
            });
        }
        let amount = self.amount;
        self.amount = 0;
        Ok(ResourceContainer::Stealth {
            address: XTR,
            revealed_amount: amount.into(),
            locked_amount: Amount::zero(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ValidatorFeeWithdrawal {
    pub address: ValidatorFeePoolAddress,
    pub amount: u64,
}
