// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

//! The exhaust burn rate as an indexer can see it.
//!
//! A validator reads the rate off the block header of the epoch a transaction is sequenced in. An
//! indexer follows state rather than blocks, and the epoch checkpoints it syncs carry the layer-1
//! shaped header, which holds a metadata hash rather than the extra data the rate lives in. So it
//! resolves the rate the same way a validator does at an epoch boundary: from the governance
//! component, which it can fetch like any other substate.

use log::*;
use tari_engine_types::{
    fees::{ExhaustBurnRate, ExhaustBurnRateSchedule, resolve_exhaust_burn_rate},
    substate::{SubstateId, SubstateValue},
};
use tari_ootle_common_types::{Epoch, SubstateRequirementRef};
use tari_ootle_transaction::Network;
use tari_template_lib_types::{constants::BURN_RATE_GOVERNANCE_COMPONENT_ADDRESS, governance::BurnRateGovernanceState};

use crate::substate_manager::SubstateManager;

const LOG_TARGET: &str = "tari::indexer::exhaust_burn_rate";

/// The rate `epoch` runs at.
///
/// Reporting and dry-run estimation only, so a component that cannot be fetched or decoded falls
/// back to [`ExhaustBurnRateSchedule`] rather than failing the caller. The burn is a share of what
/// was collected and never part of what a transaction is charged, so a stale rate moves what a dry
/// run reports as burned and nothing a caller has to pay.
pub async fn resolve_exhaust_burn_rate_for_epoch(
    substate_manager: &SubstateManager,
    network: Network,
    epoch: Epoch,
) -> ExhaustBurnRate {
    match read_governance_rate(substate_manager, epoch).await {
        Ok(governance) => resolve_exhaust_burn_rate(network, epoch, governance),
        Err(err) => {
            debug!(
                target: LOG_TARGET,
                "Burn rate governance component unavailable ({err}). Reporting {epoch} at the scheduled rate."
            );
            ExhaustBurnRateSchedule::at(network, epoch)
        },
    }
}

async fn read_governance_rate(
    substate_manager: &SubstateManager,
    epoch: Epoch,
) -> Result<Option<ExhaustBurnRate>, anyhow::Error> {
    let substate_id = SubstateId::Component(BURN_RATE_GOVERNANCE_COMPONENT_ADDRESS);
    let substate = substate_manager
        .get_substate(SubstateRequirementRef::unversioned(&substate_id))
        .await?;

    let SubstateValue::Component(component) = substate.substate_value() else {
        anyhow::bail!("{substate_id} is not a component");
    };

    let state: BurnRateGovernanceState = tari_bor::from_value(component.state())?;
    Ok(state.rate_at(epoch.as_u64()).and_then(ExhaustBurnRate::try_new))
}
