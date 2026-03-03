//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::{Network, ToSubstateAddress, VersionedSubstateId};
use tari_state_store_rocksdb::{column_families::substate::SubstateCf, writer::DbWriteContext};
use tari_template_lib::types::constants::{TOKEN_SYMBOL, XTR};

/// This migration sets the token symbol to (t)TARI -
/// TODO: squash this on testnet reset
pub fn migrate(db: &DbWriteContext<'_>, network: Network) -> anyhow::Result<()> {
    const OPERATION: &str = "v1 migration";
    let cf = db.cf(SubstateCf)?;
    let tari_resx = VersionedSubstateId::new(XTR, 0).to_substate_address();

    let mut resx = cf.get(&tari_resx, OPERATION)?;
    let tari_mut = resx
        .substate_value
        .as_mut()
        .expect("Tari XTR resource must exist")
        .as_resource_mut()
        .expect("Tari XTR resource must be a resource");
    let symbol = if network.is_testnet() { "tTARI" } else { "TARI" };
    tari_mut.metadata_mut().insert(TOKEN_SYMBOL, symbol);

    cf.put(&tari_resx, &resx, OPERATION)?;
    Ok(())
}
