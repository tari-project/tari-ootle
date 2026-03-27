//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use prometheus_client::{metrics::counter::Counter, registry::Registry};
use tari_ootle_transaction::{Transaction, TransactionId};

use crate::metrics::CollectorRegister;

#[derive(Debug, Clone)]
pub struct PrometheusMempoolMetrics {
    transactions_received: Counter,
    transaction_validation_error: Counter,
}

impl PrometheusMempoolMetrics {
    pub fn new(registry: &mut Registry) -> Self {
        let registry = registry.sub_registry_with_prefix("mempool");
        Self {
            transactions_received: Counter::default().register_at(
                "transactions_received",
                "Number of transactions received",
                registry,
            ),
            transaction_validation_error: Counter::default().register_at(
                "transaction_validation_error",
                "Number of transaction validation errors",
                registry,
            ),
        }
    }

    pub fn on_transaction_received(&mut self, _transaction: &Transaction) {
        self.transactions_received.inc();
    }

    pub fn on_transaction_validation_error<E: ToString>(&mut self, _transaction: &TransactionId, _err: &E) {
        self.transaction_validation_error.inc();
    }
}
