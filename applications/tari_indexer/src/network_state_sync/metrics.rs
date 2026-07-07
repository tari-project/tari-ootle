//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use prometheus_client::{metrics::gauge::Gauge, registry::Registry};
use tari_template_lib_types::Amount;

use crate::{metrics::CollectorRegister, store::XtrEconomics};

/// Network-wide XTR economic gauges, populated from the persisted sync totals each sync round. Gauges
/// (not counters) are set directly from the storage totals, so they are self-correcting and restart-safe;
/// Grafana derives supply, realized rate and the header-vs-receipt reconciliation from them.
#[derive(Clone)]
pub struct NetworkStateMetrics {
    exhaust_burned: Gauge,
    claimed: Gauge,
    fee_volume: Gauge,
    receipt_exhaust_burned: Gauge,
    transaction_receipt_count: Gauge,
    exhaust_rate_bps: Gauge,
}

impl NetworkStateMetrics {
    pub fn register(registry: &mut Registry) -> Self {
        let registry = registry.sub_registry_with_prefix("network");
        Self {
            exhaust_burned: Gauge::default().register_at(
                "exhaust_burned_microtari",
                "Total exhaust burned (microTARI), sourced from checkpoint headers; authoritative and complete since \
                 genesis",
                registry,
            ),
            claimed: Gauge::default().register_at(
                "claimed_microtari",
                "Total XTR claimed / pegged in (microTARI)",
                registry,
            ),
            fee_volume: Gauge::default().register_at(
                "fee_volume_microtari",
                "Total pre-burn execution fees F (microTARI), summed from transaction receipts. On an existing \
                 indexer db this accumulates from the upgrade sync frontier (go-forward only)",
                registry,
            ),
            receipt_exhaust_burned: Gauge::default().register_at(
                "receipt_exhaust_burned_microtari",
                "Total exhaust burned (microTARI) summed from the same receipts as fee_volume; receipt_exhaust_burned \
                 / fee_volume is the exact realized rate. Trails exhaust_burned when receipts are unobserved \
                 (pruned/lagging)",
                registry,
            ),
            transaction_receipt_count: Gauge::default().register_at(
                "transaction_receipt_count",
                "Number of transaction receipts the indexer has stored",
                registry,
            ),
            exhaust_rate_bps: Gauge::default().register_at(
                "exhaust_rate_bps",
                "Target exhaust burn rate in basis points in effect at the current epoch",
                registry,
            ),
        }
    }

    pub fn update(&self, economics: &XtrEconomics, target_burn_rate_bps: u16) {
        self.exhaust_burned.set(amount_to_gauge(economics.total_exhaust_burned));
        self.claimed.set(amount_to_gauge(economics.total_claimed));
        self.fee_volume.set(amount_to_gauge(economics.fee_volume));
        self.receipt_exhaust_burned
            .set(amount_to_gauge(economics.receipt_exhaust_burned));
        self.transaction_receipt_count
            .set(i64::try_from(economics.transaction_receipt_count).unwrap_or(i64::MAX));
        self.exhaust_rate_bps.set(i64::from(target_burn_rate_bps));
    }
}

/// microTARI totals stay well within `i64` (max supply ~2.1e16 << i64::MAX); clamp defensively.
fn amount_to_gauge(amount: Amount) -> i64 {
    i64::try_from(amount.to_u128()).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use prometheus_client::encoding::text::encode;

    use super::*;

    #[test]
    fn registers_and_publishes_all_gauges() {
        let mut registry = Registry::default();
        let metrics = NetworkStateMetrics::register(&mut registry);

        metrics.update(
            &XtrEconomics {
                total_claimed: Amount::from(1_000u64),
                total_exhaust_burned: Amount::from(50u64),
                fee_volume: Amount::from(800u64),
                receipt_exhaust_burned: Amount::from(40u64),
                transaction_receipt_count: 7,
            },
            500,
        );

        let mut out = String::new();
        encode(&mut out, &registry).unwrap();

        assert!(out.contains("network_claimed_microtari 1000"), "{out}");
        assert!(out.contains("network_exhaust_burned_microtari 50"), "{out}");
        assert!(out.contains("network_fee_volume_microtari 800"), "{out}");
        assert!(out.contains("network_receipt_exhaust_burned_microtari 40"), "{out}");
        assert!(out.contains("network_transaction_receipt_count 7"), "{out}");
        assert!(out.contains("network_exhaust_rate_bps 500"), "{out}");
    }
}
