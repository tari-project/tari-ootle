//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::{
    fees::{FeeBreakdown, FeeSource},
    resource_container::ResourceContainer,
};
use tari_template_lib::types::{VaultId, constants::XTR};

use crate::runtime::RuntimeError;

#[derive(Debug, Clone, Default)]
pub struct FeeState {
    fee_payments_without_refund: Vec<ResourceContainer>,
    /// The fee payments made by the user, used to pay for the transaction fees with the return vault.
    fee_payments: Vec<(ResourceContainer, VaultId)>,
    running_payments_total: u64,
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
        match self.running_payments_total.checked_add(amount) {
            Some(new_total) => self.running_payments_total = new_total,
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

    pub fn is_paid_in_full(&self) -> bool {
        self.total_payments() >= self.total_charges()
    }

    pub fn total_charges(&self) -> u64 {
        self.fee_charges.get_total()
    }

    pub fn total_payments(&self) -> u64 {
        self.running_payments_total
    }
}

#[cfg(test)]
mod tests {
    use tari_template_lib::types::{ObjectKey, ResourceAddress};

    use super::*;

    #[test]
    fn it_prevents_fees_from_exceeding_u64_max() {
        let mut fee_state = FeeState::new();
        let resource = ResourceContainer::stealth(XTR, 100u64.into());
        let vault_id = VaultId::new(Default::default());
        fee_state
            .add_fee_payment_checked(ResourceContainer::stealth(XTR, u128::MAX.into()), Some(vault_id))
            .unwrap_err();

        fee_state.add_fee_payment_checked(resource, Some(vault_id)).unwrap();
        fee_state
            .add_fee_payment_checked(ResourceContainer::stealth(XTR, 123u64.into()), Some(vault_id))
            .unwrap();

        // 1 more than u64::MAX when added to previous payments
        fee_state
            .add_fee_payment_checked(
                ResourceContainer::stealth(XTR, (u64::MAX - 223 + 1).into()),
                Some(vault_id),
            )
            .unwrap_err();
        assert_eq!(fee_state.total_payments(), 100 + 123);
    }

    #[test]
    fn it_errors_if_incorrect_fee_resource_used() {
        let mut fee_state = FeeState::new();
        let resource = ResourceAddress::new(ObjectKey::default());
        assert_ne!(resource, XTR);
        let resource = ResourceContainer::stealth(resource, 100u64.into());
        let err = fee_state.add_fee_payment_checked(resource, None).unwrap_err();
        assert!(matches!(err, RuntimeError::InvalidArgument { .. }));
    }

    #[test]
    fn it_tracks_refundable_payments() {
        let mut fee_state = FeeState::new();
        let resource = ResourceContainer::stealth(XTR, 100u64.into());
        let vault_id = VaultId::new(Default::default());
        fee_state
            .add_fee_payment_checked(resource.clone(), Some(vault_id))
            .unwrap();
        let mut drained: Vec<_> = fee_state.refundable_fee_payments_iter_mut().collect();
        assert_eq!(drained.len(), 1);
        let (drained_resource, drained_vault_id) = drained.pop().unwrap();
        assert_eq!(drained_resource.unlocked_amount(), resource.unlocked_amount());
        assert_eq!(*drained_vault_id, vault_id);
    }

    #[test]
    fn it_determines_if_fees_are_paid_in_full_with_refunds() {
        let mut fee_state = FeeState::new();
        fee_state.add_charge(FeeSource::Initial, 100);
        assert_eq!(fee_state.total_charges(), 100);
        assert!(!fee_state.is_paid_in_full());

        // First payment
        let resource = ResourceContainer::stealth(XTR, 10u64.into());
        let vault_id = VaultId::new(Default::default());
        fee_state.add_fee_payment_checked(resource, Some(vault_id)).unwrap();
        assert!(!fee_state.is_paid_in_full());

        // Second payment
        let resource = ResourceContainer::stealth(XTR, 1000u64.into());
        let vault_id = VaultId::new(Default::default());
        fee_state.add_fee_payment_checked(resource, Some(vault_id)).unwrap();
        assert!(fee_state.is_paid_in_full());

        // Assert
        let mut iter = fee_state.refundable_fee_payments_iter_mut();
        let (refund, vault) = iter.next().unwrap();
        assert_eq!(refund.unlocked_amount(), 10);
        assert_eq!(*vault, vault_id);

        let (refund, vault) = iter.next().unwrap();
        assert_eq!(refund.unlocked_amount(), 1000);
        assert_eq!(*vault, vault_id);

        assert!(iter.next().is_none());
    }
}
