//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::{
    fees::{FeeBreakdown, FeeSource},
    resource_container::ResourceContainer,
};
use tari_template_lib::{models::VaultId, prelude::XTR};

use crate::runtime::RuntimeError;

#[derive(Debug, Clone, Default)]
pub struct FeeState {
    fee_payments_without_refund: Vec<ResourceContainer>,
    /// The fee payments made by the user, used to pay for the transaction fees with the return vault.
    fee_payments: Vec<(ResourceContainer, VaultId)>,
    running_total: u64,
    fee_charges: FeeBreakdown,
}

impl FeeState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_fee_payment_checked(
        &mut self,
        resource_container: ResourceContainer,
        vault_id: Option<VaultId>,
    ) -> Result<(), RuntimeError> {
        if *resource_container.resource_address() != XTR {
            return Err(RuntimeError::InvalidArgument {
                argument: "vault_ref",
                reason: format!(
                    "Fees can only be paid using XTR, however the vault contained resource {}",
                    resource_container.resource_address()
                ),
            });
        }

        let Some(amount) = resource_container.unlocked_amount().to_u64_checked() else {
            return Err(RuntimeError::InvalidAmount {
                amount: resource_container.unlocked_amount(),
                reason: "Payed an invalid amount. Amount must be positive and not overflow".to_string(),
            });
        };
        match self.running_total.checked_add(amount) {
            Some(new_total) => self.running_total = new_total,
            None => {
                return Err(RuntimeError::InvalidAmount {
                    amount: resource_container.unlocked_amount(),
                    reason: "Payed an invalid amount. Amount overflowed".to_string(),
                });
            },
        }
        if let Some(vault_id) = vault_id {
            self.fee_payments.push((resource_container, vault_id));
        } else {
            self.fee_payments_without_refund.push(resource_container);
        }
        Ok(())
    }

    pub fn drain_refundable_fee_payments(&mut self) -> impl Iterator<Item = (ResourceContainer, VaultId)> + '_ {
        self.fee_payments.drain(..)
    }

    pub fn refundable_fee_payments_iter_mut(
        &mut self,
    ) -> impl Iterator<Item = (&mut ResourceContainer, &mut VaultId)> + '_ {
        self.fee_payments.iter_mut().map(|(rc, vid)| (rc, vid))
    }

    pub fn non_refundable_fee_payments_mut_iter(&mut self) -> impl Iterator<Item = &mut ResourceContainer> + '_ {
        self.fee_payments_without_refund.iter_mut()
    }

    pub fn add_charge(&mut self, source: FeeSource, amount: u64) {
        self.fee_charges.add(source, amount)
    }

    pub fn take_fee_charges(&mut self) -> FeeBreakdown {
        std::mem::take(&mut self.fee_charges)
    }

    pub fn total_charges(&self) -> u64 {
        self.fee_charges.get_total()
    }

    pub fn total_payments(&self) -> u64 {
        self.running_total
    }
}
