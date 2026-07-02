//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#[cfg(test)]
mod balance_change_tests {
    use tari_template_lib::models::Amount;
    use crate::models::balance_change::{BalanceChange, BalanceChangeSource};

    fn dummy_vault_id() -> tari_template_lib::models::VaultId {
        tari_template_lib::models::VaultId::from([0u8; 32])
    }
    fn dummy_addr() -> tari_template_lib::models::ComponentAddress {
        tari_template_lib::models::ComponentAddress::from([0u8; 32])
    }
    fn dummy_resource() -> tari_template_lib::models::ResourceAddress {
        tari_template_lib::models::ResourceAddress::from([0u8; 32])
    }

    #[test]
    fn positive_delta_when_balance_increases() {
        let change = BalanceChange::new(
            dummy_vault_id(), dummy_addr(), dummy_resource(),
            Amount::from(1_000u64), Amount::zero(),
            Amount::from(2_000u64), Amount::zero(),
            BalanceChangeSource::Scan,
        );
        assert_eq!(change.revealed_delta, 1_000i128);
        assert!(change.has_change());
    }

    #[test]
    fn negative_delta_when_balance_decreases() {
        // Critical: this must not panic (was the original bug - Amount underflow)
        let change = BalanceChange::new(
            dummy_vault_id(), dummy_addr(), dummy_resource(),
            Amount::from(5_000u64), Amount::zero(),
            Amount::from(3_000u64), Amount::zero(),
            BalanceChangeSource::Scan,
        );
        assert_eq!(change.revealed_delta, -2_000i128,
            "Decrease must produce negative i128 delta without panic");
    }

    #[test]
    fn zero_delta_when_balance_unchanged() {
        let change = BalanceChange::new(
            dummy_vault_id(), dummy_addr(), dummy_resource(),
            Amount::from(1_000u64), Amount::zero(),
            Amount::from(1_000u64), Amount::zero(),
            BalanceChangeSource::Scan,
        );
        assert_eq!(change.revealed_delta, 0i128);
        assert!(!change.has_change());
    }

    #[test]
    fn transaction_source_stores_id() {
        use tari_ootle_transaction::transaction_id::TransactionId;
        let tx_id = TransactionId::default();
        let source = BalanceChangeSource::Transaction { transaction_id: tx_id.clone() };
        assert_eq!(source.source_tag(), 0);
        assert_eq!(source.transaction_id(), Some(&tx_id));
    }

    #[test]
    fn scan_source_has_no_tx_id() {
        let source = BalanceChangeSource::Scan;
        assert_eq!(source.source_tag(), 1);
        assert!(source.transaction_id().is_none());
    }

    #[test]
    fn recovery_source_has_no_tx_id() {
        let source = BalanceChangeSource::Recovery;
        assert_eq!(source.source_tag(), 2);
        assert!(source.transaction_id().is_none());
    }
}
