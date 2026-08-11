//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use indexmap::{IndexMap, map::Entry};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default)]
pub struct FeeReceiptBuilder {
    /// The total amount of the fee payment(s)
    pub total_fee_payment: u64,
    /// Total fees paid after refunds
    pub total_fees_paid: u64,
    /// The amount of non-refundable fees which the user overpaid. Fees cannot be refunded when paying purely with a
    /// stealth reveal (since we do not know the account/vault to refund).
    pub total_fee_overcharge: u64,
    /// Breakdown of fee costs
    pub cost_breakdown: FeeBreakdown,
}

impl FeeReceiptBuilder {
    pub fn with_total_fee_payment(mut self, amount: u64) -> Self {
        self.total_fee_payment = amount;
        self
    }

    pub fn with_total_fees_paid(mut self, amount: u64) -> Self {
        self.total_fees_paid = amount;
        self
    }

    pub fn with_total_fee_overcharge(mut self, amount: u64) -> Self {
        self.total_fee_overcharge = amount;
        self
    }

    pub fn with_cost_breakdown(mut self, breakdown: FeeBreakdown) -> Self {
        self.cost_breakdown = breakdown;
        self
    }

    pub fn build(self) -> FeeReceipt {
        FeeReceipt {
            total_fee_payment: self.total_fee_payment,
            total_fees_paid: self.total_fees_paid,
            total_fee_overcharge: self.total_fee_overcharge,
            cost_breakdown: self.cost_breakdown,
        }
    }
}

#[derive(
    Debug, Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen, Serialize, Deserialize, borsh::BorshSerialize,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FeeReceipt {
    /// The total amount of the fee payment(s)
    #[n(0)]
    total_fee_payment: u64,
    /// Total fees paid after refunds
    #[n(1)]
    total_fees_paid: u64,
    /// The amount of non-refundable fees which the user overpaid. Fees cannot be refunded when paying purely with a
    /// stealth reveal (since we do not know the account/vault to refund).
    #[n(2)]
    total_fee_overcharge: u64,
    /// Breakdown of fee costs
    #[n(3)]
    cost_breakdown: FeeBreakdown,
}

impl FeeReceipt {
    pub fn builder() -> FeeReceiptBuilder {
        FeeReceiptBuilder::default()
    }

    /// The widest form this type can encode to: every amount at full varint width and a breakdown
    /// entry for every [`FeeSource`]. Bounds the encoded size of a receipt whose fees are not yet
    /// settled — see `TransactionReceipt::encoded_size_upper_bound`.
    pub fn widest() -> Self {
        let mut cost_breakdown = FeeBreakdown::default();
        for source in FeeSource::ALL {
            cost_breakdown.add(source, u64::MAX);
        }
        Self {
            total_fee_payment: u64::MAX,
            total_fees_paid: u64::MAX,
            total_fee_overcharge: u64::MAX,
            cost_breakdown,
        }
    }

    pub fn to_cost_breakdown(&self) -> FeeCostBreakdown {
        FeeCostBreakdown {
            total_fees_charged: self.total_fees_charged(),
            required_fees: self.required_fees(),
            breakdown: self.cost_breakdown.clone(),
        }
    }

    pub fn fee_breakdown(&self) -> &FeeBreakdown {
        &self.cost_breakdown
    }

    /// The total amount of fees charged. This may be more than total_fees_paid if the user paid an insufficient amount.
    pub fn total_fees_charged(&self) -> u64 {
        self.cost_breakdown.get_total()
    }

    /// The minimum fee required to submit a transaction based on a dry run result.
    /// This is `total_fees_charged + 1` to account for potential rounding differences in the storage cost calculation.
    /// The storage fee depends on the vault balance at calculation time, which changes when a different max_fee is used
    /// in the actual submission vs the dry run — this can shift `floor(total_bytes / 4)` by 1 at a rounding boundary.
    pub fn required_fees(&self) -> u64 {
        self.total_fees_charged().saturating_add(1)
    }

    /// The total amount of fees refunded to the respective vaults
    pub fn total_refunded(&self) -> u64 {
        self.total_fee_payment
            .checked_sub(self.total_fees_charged())
            // Minus overcharge (funds that cannot be refunded)
            .and_then(|v| v.checked_sub(self.total_fee_overcharge))
            .unwrap_or_default()
    }

    /// The total amount of fees allocated to the transaction, before refunds
    pub fn total_allocated_fee_payments(&self) -> u64 {
        self.total_fee_payment
    }

    /// The total amount of fees paid after refunds
    pub fn total_fees_paid(&self) -> u64 {
        self.total_fees_paid
    }

    /// The total amount of the fee payment(s) before refunds.
    pub fn total_fee_payment(&self) -> u64 {
        self.total_fee_payment
    }

    /// The amount of unpaid fees
    pub fn unpaid_debt(&self) -> u64 {
        self.total_fees_charged().saturating_sub(self.total_fees_paid())
    }

    /// Returns true if the total fees charged is less than or equal to the total fees paid, otherwise false
    pub fn is_paid_in_full(&self) -> bool {
        self.unpaid_debt() == 0
    }

    /// The amount of non-refundable fees which the user overpaid. Fees cannot be refunded when paying purely with a
    /// stealth reveal (since we do not know the account/vault to refund).
    pub fn total_fee_overcharge(&self) -> u64 {
        self.total_fee_overcharge
    }

    /// The exhaust burn charged on top of the execution fee.
    pub fn exhaust_burn_charged(&self) -> u64 {
        self.cost_breakdown.get(FeeSource::ExhaustBurn)
    }

    /// The total amount of fees paid after refunds, excluding the exhaust burn. This is the execution cost `F` that
    /// flows to leaders in full; the burn portion is destroyed.
    pub fn pre_burn_fees_paid(&self) -> u64 {
        self.total_fees_paid().saturating_sub(self.exhaust_burn_charged())
    }
}

impl Default for FeeReceipt {
    fn default() -> Self {
        FeeReceiptBuilder::default().build()
    }
}

#[repr(u8)]
#[derive(
    Debug,
    Clone,
    Copy,
    minicbor::Encode,
    minicbor::Decode,
    minicbor::CborLen,
    Serialize,
    Deserialize,
    Hash,
    Eq,
    PartialEq,
    PartialOrd,
    Ord,
    borsh::BorshSerialize,
)]
#[borsh(use_discriminant = true)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum FeeSource {
    #[n(0)]
    Initial = 0,
    #[n(1)]
    RuntimeCall = 1,
    #[n(2)]
    Storage = 2,
    #[n(3)]
    TransactionWeight = 3,
    #[n(4)]
    SignatureVerification = 4,
    #[n(5)]
    TemplateLoad = 5,
    #[n(6)]
    SubstateCreate = 6,
    /// WASM execution metering, charged in proportion to consumed Wasmer metering points.
    #[n(7)]
    WasmExecution = 7,
    /// Cost of publishing a template's binary, replacing the flat per-byte `Storage` charge for
    /// that binary: the first `template_size_premium_free_bytes` are priced at the per-byte storage
    /// rate, and every whole unit beyond that is charged quadratically to discourage oversized
    /// templates.
    #[n(8)]
    TemplatePublish = 8,
    /// Exhaust burn, charged on top of the execution fee and destroyed rather than paid to leaders.
    #[n(9)]
    ExhaustBurn = 9,
    /// Native verification metering (stealth transfers, confidential withdraws, burn claims),
    /// priced in the same points as `WasmExecution` via wall-clock equivalence and charged at the
    /// same per-point rate.
    #[n(10)]
    NativeExecution = 10,
}

impl FeeSource {
    /// Every variant. `fee_source_all_is_exhaustive` fails to compile if a variant is added without
    /// being listed here.
    pub const ALL: [Self; 11] = [
        Self::Initial,
        Self::RuntimeCall,
        Self::Storage,
        Self::TransactionWeight,
        Self::SignatureVerification,
        Self::TemplateLoad,
        Self::SubstateCreate,
        Self::WasmExecution,
        Self::TemplatePublish,
        Self::ExhaustBurn,
        Self::NativeExecution,
    ];
}

#[derive(
    Debug,
    Clone,
    minicbor::Encode,
    minicbor::Decode,
    minicbor::CborLen,
    Serialize,
    Deserialize,
    Default,
    borsh::BorshSerialize,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FeeBreakdown {
    #[n(0)]
    #[cbor(with = "tari_bor::adapters::indexmap_codec")]
    breakdown: IndexMap<FeeSource, u64>,
}

impl FeeBreakdown {
    pub fn add(&mut self, source: FeeSource, amount: u64) {
        match self.breakdown.entry(source) {
            Entry::Occupied(entry) => {
                *entry.into_mut() += amount;
            },
            Entry::Vacant(entry) => {
                entry.insert(amount);
                self.breakdown.sort_keys();
            },
        }
    }

    /// Returns an iterator over the fee breakdown in a canonical order.
    pub fn iter(&self) -> impl Iterator<Item = (&FeeSource, &u64)> {
        self.breakdown.iter()
    }

    pub fn get_total(&self) -> u64 {
        self.breakdown.values().sum()
    }

    pub fn get(&self, source: FeeSource) -> u64 {
        self.breakdown.get(&source).copied().unwrap_or_default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FeeCostBreakdown {
    pub total_fees_charged: u64,
    pub required_fees: u64,
    pub breakdown: FeeBreakdown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fee_source_all_is_exhaustive() {
        for source in FeeSource::ALL {
            // An added variant fails to compile here until it is listed in `FeeSource::ALL`.
            match source {
                FeeSource::Initial |
                FeeSource::RuntimeCall |
                FeeSource::Storage |
                FeeSource::TransactionWeight |
                FeeSource::SignatureVerification |
                FeeSource::TemplateLoad |
                FeeSource::SubstateCreate |
                FeeSource::WasmExecution |
                FeeSource::TemplatePublish |
                FeeSource::ExhaustBurn |
                FeeSource::NativeExecution => {},
            }
        }
    }

    #[test]
    fn widest_bounds_every_other_receipt() {
        let widest = minicbor::len(FeeReceipt::widest());

        let mut breakdown = FeeBreakdown::default();
        breakdown.add(FeeSource::Initial, 1000);
        breakdown.add(FeeSource::Storage, u64::MAX);
        let realistic = FeeReceipt::builder()
            .with_total_fee_payment(u64::MAX)
            .with_total_fees_paid(u64::MAX)
            .with_total_fee_overcharge(u64::MAX)
            .with_cost_breakdown(breakdown)
            .build();

        assert!(minicbor::len(&realistic) <= widest);
        assert!(minicbor::len(FeeReceipt::default()) <= widest);
    }
}
