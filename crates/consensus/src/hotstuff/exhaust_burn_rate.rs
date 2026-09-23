// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

//! Resolving the exhaust burn rate an epoch runs at.
//!
//! The rate reaches execution through the block header — `LockedEpoch` reads it off the block a
//! transaction is sequenced in — so it is resolved from state exactly twice per epoch: once by the
//! leader proposing the end-of-epoch block that opens the next epoch, and once by every replica
//! voting on that block. Between those two points it is a header field, never a substate read.
//!
//! Every node resolves the same value because the answer depends only on the epoch and on the
//! governance component, which lives on the global shard and so is held by every shard group.

use log::*;
use tari_engine_types::{
    fees::{ExhaustBurnRate, resolve_exhaust_burn_rate},
    substate::{SubstateId, SubstateValue},
};
use tari_ootle_common_types::{Epoch, optional::Optional};
use tari_ootle_storage::{StateStoreReadTransaction, consensus_models::SubstateRecord};
use tari_ootle_transaction::Network;
use tari_template_lib_types::{constants::BURN_RATE_GOVERNANCE_COMPONENT_ADDRESS, governance::BurnRateGovernanceState};

use crate::hotstuff::HotStuffError;

const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::exhaust_burn_rate";

/// The exhaust burn rate `epoch` runs at.
///
/// Reads committed state rather than anything pending, which is sound because a council transaction
/// cannot name an activation epoch less than `MIN_BURN_RATE_ACTIVATION_LEAD_EPOCHS` ahead of the
/// epoch it executes in. The end-of-epoch block of epoch `N` resolves the rate for `N + 1`, so every
/// change that can reach `N + 1` was committed in `N - 1` at the latest.
pub fn resolve_epoch_exhaust_burn_rate<TTx: StateStoreReadTransaction>(
    tx: &TTx,
    network: Network,
    epoch: Epoch,
) -> Result<ExhaustBurnRate, HotStuffError> {
    let governance = read_governance_rate(tx, epoch)?;
    Ok(resolve_exhaust_burn_rate(network, epoch, governance))
}

/// The rate the governance component holds for `epoch`.
///
/// `None` covers a council that has scheduled nothing reaching `epoch` and a component that is
/// absent, destroyed or holding something this binary cannot decode. All of them mean the same thing
/// to the caller — the table governs — and none of them may stop an epoch opening, which is why a
/// component that will not decode is logged and treated as silent rather than propagated.
fn read_governance_rate<TTx: StateStoreReadTransaction>(
    tx: &TTx,
    epoch: Epoch,
) -> Result<Option<ExhaustBurnRate>, HotStuffError> {
    let substate_id = SubstateId::Component(BURN_RATE_GOVERNANCE_COMPONENT_ADDRESS);
    let Some(record) = SubstateRecord::get_latest(tx, &substate_id).optional()? else {
        return Ok(None);
    };

    let Some(SubstateValue::Component(component)) = record.substate_value() else {
        return Ok(None);
    };

    let state: BurnRateGovernanceState = match tari_bor::from_value(component.state()) {
        Ok(state) => state,
        Err(err) => {
            warn!(
                target: LOG_TARGET,
                "🔥 Burn rate governance component did not decode ({err}). Resolving {epoch} from the rate schedule."
            );
            return Ok(None);
        },
    };

    Ok(state.rate_at(epoch.as_u64()).and_then(ExhaustBurnRate::try_new))
}
