//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::{
    fees::{FeeBreakdown, FeeSource},
    resource_container::ResourceContainer,
};
use tari_template_lib::types::{VaultId, constants::TARI_TOKEN};

use crate::runtime::RuntimeError;

#[derive(Debug, Clone, Default)]
pub struct FeeState {
    fee_payments_without_refund: Vec<ResourceContainer>,
    /// The fee payments made by the user, used to pay for the transaction fees with the return vault.
    fee_payments: Vec<(ResourceContainer, VaultId)>,
    running_payments_total: u64,
    fee_charges: FeeBreakdown,
    /// Raw Wasmer metering points consumed across every WASM invocation in this transaction.
    /// Summed across invocations so the divisor in `FeeModule::on_before_finalize` rounds once
    /// against the total — dividing per-call would let small invocations round to zero and let a
    /// caller evade WASM fees by splitting work below the divisor.
    accumulated_wasm_points: u64,
    /// Native-verification metering points charged across the transaction (stealth transfers,
    /// confidential withdraws, burn claims), priced in WASM-point equivalents. Kept separate from
    /// `accumulated_wasm_points` because the per-transaction WASM hard cap bounds WASM work only
    /// (native work has its own structural caps), while the payment-funded allowance bounds the
    /// sum of both.
    accumulated_native_points: u64,
    /// When true, fee charges are still metered but the executor will not abort if payments are insufficient.
    /// Set out-of-band by `FeeModule` during runtime initialization (only ever true for indexer dry-runs).
    dry_run: bool,
    /// The exhaust burn rate in basis points, resolved for the execution epoch. Set out-of-band by `FeeModule`
    /// during runtime initialization; used at settlement to take the burn share out of any non-refundable fee
    /// overcharge.
    burn_rate_bps: u16,
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
        if *resource_container.resource_address() != TARI_TOKEN {
            return Err(RuntimeError::InvalidArgument {
                argument: "vault_ref",
                reason: format!(
                    "Fees can only be paid using TARI, however the vault contained resource {}",
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

    /// Replaces the charge for `source`. Used by the fee module to recompute the finalization
    /// charges once the state that will actually be persisted is known.
    pub fn set_charge(&mut self, source: FeeSource, amount: u64) {
        self.fee_charges.set(source, amount)
    }

    pub fn accumulate_wasm_points(&mut self, points: u64) {
        self.accumulated_wasm_points = self.accumulated_wasm_points.saturating_add(points);
    }

    pub fn accumulated_wasm_points(&self) -> u64 {
        self.accumulated_wasm_points
    }

    pub fn accumulate_native_points(&mut self, points: u64) {
        self.accumulated_native_points = self.accumulated_native_points.saturating_add(points);
    }

    pub fn accumulated_native_points(&self) -> u64 {
        self.accumulated_native_points
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

    pub fn set_dry_run(&mut self, dry_run: bool) {
        self.dry_run = dry_run;
    }

    pub fn is_dry_run(&self) -> bool {
        self.dry_run
    }

    pub fn set_burn_rate_bps(&mut self, rate_bps: u16) {
        self.burn_rate_bps = rate_bps;
    }

    pub fn burn_rate_bps(&self) -> u16 {
        self.burn_rate_bps
    }
}

/// Splits a non-refundable fee overcharge into `(validator_share, exhaust_burn_share)`, treating the overcharge as
/// a payment inclusive of its own burn (`validator_share * (1 + rate) ~= overcharge`). This keeps the burn that
/// consensus carries from the pre-burn transaction fee consistent (to within integer rounding) with the amount
/// actually withheld from validators.
pub(crate) fn split_overcharge(overcharge: u64, rate_bps: u16) -> (u64, u64) {
    let validator_share = (u128::from(overcharge) * 10_000 / (10_000 + u128::from(rate_bps))) as u64;
    (validator_share, overcharge - validator_share)
}

#[cfg(test)]
mod tests {
    use tari_template_lib::types::{ObjectKey, ResourceAddress};

    use super::*;

    #[test]
    fn it_splits_the_overcharge_as_a_burn_inclusive_payment() {
        assert_eq!(split_overcharge(0, 500), (0, 0));
        assert_eq!(split_overcharge(100, 0), (100, 0));
        // 105 = 100 validator share + 5% burn on it
        assert_eq!(split_overcharge(105, 500), (100, 5));

        for overcharge in [1u64, 99, 100, 101, 5_000, 123_456_789, u64::MAX] {
            for rate_bps in [0u16, 1, 250, 500, 10_000] {
                let (validator_share, burn_share) = split_overcharge(overcharge, rate_bps);
                assert_eq!(
                    validator_share + burn_share,
                    overcharge,
                    "shares must sum to the overcharge (overcharge: {overcharge}, rate_bps: {rate_bps})"
                );
                let burn_on_share = (u128::from(validator_share) * u128::from(rate_bps) / 10_000) as u64;
                assert!(
                    burn_share.abs_diff(burn_on_share) <= 1,
                    "burn share must be the burn on the validator share within rounding (overcharge: {overcharge}, \
                     rate_bps: {rate_bps}, burn_share: {burn_share}, expected: {burn_on_share})"
                );
            }
        }
    }

    #[test]
    fn it_prevents_fees_from_exceeding_u64_max() {
        let mut fee_state = FeeState::new();
        let resource = ResourceContainer::stealth(TARI_TOKEN, 100u64.into());
        let vault_id = VaultId::new(Default::default());
        fee_state
            .add_fee_payment_checked(ResourceContainer::stealth(TARI_TOKEN, u128::MAX.into()), Some(vault_id))
            .unwrap_err();

        fee_state.add_fee_payment_checked(resource, Some(vault_id)).unwrap();
        fee_state
            .add_fee_payment_checked(ResourceContainer::stealth(TARI_TOKEN, 123u64.into()), Some(vault_id))
            .unwrap();

        // 1 more than u64::MAX when added to previous payments
        fee_state
            .add_fee_payment_checked(
                ResourceContainer::stealth(TARI_TOKEN, (u64::MAX - 223 + 1).into()),
                Some(vault_id),
            )
            .unwrap_err();
        assert_eq!(fee_state.total_payments(), 100 + 123);
    }

    #[test]
    fn it_errors_if_incorrect_fee_resource_used() {
        let mut fee_state = FeeState::new();
        let resource = ResourceAddress::new(ObjectKey::default());
        assert_ne!(resource, TARI_TOKEN);
        let resource = ResourceContainer::stealth(resource, 100u64.into());
        let err = fee_state.add_fee_payment_checked(resource, None).unwrap_err();
        assert!(matches!(err, RuntimeError::InvalidArgument { .. }));
    }

    #[test]
    fn it_tracks_refundable_payments() {
        let mut fee_state = FeeState::new();
        let resource = ResourceContainer::stealth(TARI_TOKEN, 100u64.into());
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
        let resource = ResourceContainer::stealth(TARI_TOKEN, 10u64.into());
        let vault_id = VaultId::new(Default::default());
        fee_state.add_fee_payment_checked(resource, Some(vault_id)).unwrap();
        assert!(!fee_state.is_paid_in_full());

        // Second payment
        let resource = ResourceContainer::stealth(TARI_TOKEN, 1000u64.into());
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
