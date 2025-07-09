//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::{fees::FeeBreakdown, resource_container::ResourceContainer};
use tari_template_lib::models::VaultId;

#[derive(Debug, Clone, Default)]
pub struct FeeState {
    /// The fee payments made by the user, used to pay for the transaction fees with the return vault.
    pub fee_payments: Vec<(ResourceContainer, VaultId)>,
    pub running_total: u64,
    pub fee_charges: FeeBreakdown,
}

impl FeeState {
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn add_fee_payment(&mut self, resource_container: ResourceContainer, vault_id: VaultId) -> bool {
        let Some(amount) = resource_container.amount().to_u64_checked() else {
            return false; // Invalid amount, cannot add payment
        };
        match self.running_total.checked_add(amount) {
            Some(new_total) => self.running_total = new_total,
            None => return false, // Overflow, cannot add payment
        }
        self.fee_payments.push((resource_container, vault_id));
        true
    }

    pub fn total_charges(&self) -> u64 {
        self.fee_charges.get_total()
    }

    pub fn total_payments(&self) -> u64 {
        self.running_total
    }
}
