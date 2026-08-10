//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use tari_template_lib::types::{
    Amount,
    SubstateOwnerRule,
    ValidatorFeePoolAddress,
    constants::TARI_TOKEN,
    crypto::RistrettoPublicKeyBytes,
};

use crate::{
    ownership::Ownership,
    resource_container::{ResourceContainer, ResourceError},
};

#[derive(
    Debug, Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen, Serialize, Deserialize, borsh::BorshSerialize,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ValidatorFeePool {
    #[n(0)]
    #[cfg_attr(feature = "ts", ts(type = "ArrayBuffer"))]
    pub claim_public_key: RistrettoPublicKeyBytes,
    #[n(1)]
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
            owner_rule: Cow::Owned(SubstateOwnerRule::ByPublicKey(self.claim_public_key)),
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
    pub fn withdraw_all(&mut self) -> Result<(u64, ResourceContainer), ResourceError> {
        self.withdraw_up_to(Amount::MAX)
    }

    /// Withdraws up to max_amount from the pool and returns them in a ResourceContainer.
    pub fn withdraw_up_to(&mut self, max_amount: Amount) -> Result<(u64, ResourceContainer), ResourceError> {
        if max_amount == Amount::zero() {
            return Err(ResourceError::InsufficientBalance {
                details: "Cannot withdraw zero from ValidatorFeePool".to_string(),
            });
        }
        if self.amount == 0 {
            return Err(ResourceError::InsufficientBalance {
                details: "ValidatorFeePool has insufficient balance. Current balance is 0".to_string(),
            });
        }
        let max_u64 = u64::try_from(max_amount.to_u128()).unwrap_or(u64::MAX);
        let amount = self.amount.min(max_u64);
        self.amount -= amount;
        Ok((amount, ResourceContainer::Stealth {
            address: TARI_TOKEN,
            revealed_amount: amount.into(),
            locked_amount: Amount::zero(),
        }))
    }
}

#[derive(
    Debug, Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen, Serialize, Deserialize, borsh::BorshSerialize,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ValidatorFeeWithdrawal {
    #[n(0)]
    pub address: ValidatorFeePoolAddress,
    #[n(1)]
    pub amount: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_withdraw_up_to() {
        let pk = RistrettoPublicKeyBytes::default();
        let mut pool = ValidatorFeePool::new(pk, 100);

        // 1. Cap below balance
        let (amt, container) = pool.withdraw_up_to(Amount::from(40u64)).unwrap();
        assert_eq!(amt, 40);
        assert_eq!(container.unlocked_amount(), Amount::from(40u64));
        assert_eq!(pool.amount(), 60);

        // 2. Cap at or above balance
        let (amt, container) = pool.withdraw_up_to(Amount::from(100u64)).unwrap();
        assert_eq!(amt, 60);
        assert_eq!(container.unlocked_amount(), Amount::from(60u64));
        assert_eq!(pool.amount(), 0);

        // 3. Attempt to withdraw from empty pool
        let err = pool.withdraw_up_to(Amount::from(10u64)).unwrap_err();
        assert!(matches!(err, ResourceError::InsufficientBalance { .. }));

        // 4. Attempt to withdraw 0
        let mut pool = ValidatorFeePool::new(pk, 100);
        let err = pool.withdraw_up_to(Amount::from(0u64)).unwrap_err();
        assert!(matches!(err, ResourceError::InsufficientBalance { .. }));
        assert_eq!(pool.amount(), 100); // untouched
    }
}
