//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine::fees::FeeTable;
use tari_ootle_common_types::Network;

const TESTNET_FEE_TABLE: FeeTable = FeeTable {
    per_transaction_weight_cost: 1,
    per_module_call_cost: 1,
    per_byte_storage_cost: 1,
    per_event_cost: 1,
    per_log_cost: 1,
    per_signature_verification_cost: 10,
    per_template_load_cost_unit: 1,
};

// TODO: finalize these values
const MAINNET_FEE_TABLE: FeeTable = FeeTable {
    per_transaction_weight_cost: 1,
    per_module_call_cost: 1,
    per_byte_storage_cost: 1,
    per_event_cost: 1,
    per_log_cost: 1,
    per_signature_verification_cost: 10,
    per_template_load_cost_unit: 1,
};

pub const fn get_fee_table_by_network(network: Network) -> &'static FeeTable {
    match network {
        Network::LocalNet => &TESTNET_FEE_TABLE,
        Network::Igor => &TESTNET_FEE_TABLE,
        Network::Esmeralda => &TESTNET_FEE_TABLE,
        Network::StageNet => &TESTNET_FEE_TABLE,
        Network::NextNet => &TESTNET_FEE_TABLE,
        Network::MainNet => &MAINNET_FEE_TABLE,
    }
}
