//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::ops::Deref;

use serde::Serialize;
use tari_engine_types::resource::Resource;
use tari_ootle_common_types::{Network, NodeAddressable, NumPreshards};
use tari_ootle_storage::{StateStoreReadTransaction, StateStoreWriteTransaction, StorageError};
use tari_template_lib::{
    prelude::{Metadata, ResourceAccessRules, ResourceType, rule},
    types::{
        SubstateOwnerRule,
        constants::{XTR_FAUCET_CLAIM_RESOURCE_ADDRESS, XTR_FAUCET_COMPONENT_ADDRESS},
    },
};

use crate::migrations::common::create_substate;

/// This migration adds the XTR faucet claim resource needed for the limited faucet.
pub fn migrate<TTx>(tx: &mut TTx, network: Network, num_preshards: NumPreshards) -> Result<(), StorageError>
where
    TTx: StateStoreWriteTransaction + Deref,
    TTx::Target: StateStoreReadTransaction,
    TTx::Addr: NodeAddressable + Serialize,
{
    if !network.is_testnet() {
        return Ok(());
    }

    // Add the claim receipt resource for the limited faucet (one NFT per claimant public key).
    let claim_resource = Resource::new(
        ResourceType::NonFungible,
        SubstateOwnerRule::None,
        ResourceAccessRules::new()
            .mintable(rule!(component(XTR_FAUCET_COMPONENT_ADDRESS)))
            .burnable(rule!(component(XTR_FAUCET_COMPONENT_ADDRESS))),
        Metadata::new(),
        None,
        None,
        0,
        false,
    );
    create_substate(tx, num_preshards, XTR_FAUCET_CLAIM_RESOURCE_ADDRESS, claim_resource)?;

    Ok(())
}
