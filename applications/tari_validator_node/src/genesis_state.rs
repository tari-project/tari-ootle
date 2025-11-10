//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::ops::Deref;

use serde::Serialize;
use tari_bor::cbor;
use tari_engine_types::{
    component::{ComponentBody, ComponentHeader},
    resource::Resource,
    resource_container::ResourceContainer,
    substate::{SubstateId, SubstateValue},
    vault::Vault,
};
use tari_ootle_app_utilities::shared_consts::TXTR_FAUCET_INITIAL_SUPPLY;
use tari_ootle_common_types::{
    Epoch,
    Network,
    NodeAddressable,
    NumPreshards,
    VersionedSubstateId,
    VersionedSubstateIdRef,
};
use tari_ootle_storage::{
    consensus_models::{SubstateRecord, SubstateTransition, SubstateUpdateBatch},
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
};
use tari_state_tree::Version;
use tari_template_lib::{
    auth::{ComponentAccessRules, OwnerRule, ResourceAccessRules},
    constants::{
        NFT_FAUCET_COMPONENT_ADDRESS,
        NFT_FAUCET_RESOURCE_ADDRESS,
        PUBLIC_IDENTITY_RESOURCE_ADDRESS,
        STEALTH_TARI_RESOURCE_ADDRESS,
        XTR_FAUCET_COMPONENT_ADDRESS,
        XTR_FAUCET_VAULT_ADDRESS,
    },
    models::Metadata,
    prelude::ResourceType,
    resource::TOKEN_SYMBOL,
    rule,
    types::EntityId,
};

const INITIAL_STATE_VERSION: Version = 0;

pub fn has_bootstrapped<TTx: StateStoreReadTransaction>(tx: &TTx) -> Result<bool, StorageError> {
    // Assume that if the public identity resource exists, then the rest of the state has been bootstrapped
    SubstateRecord::exists(
        tx,
        VersionedSubstateId::new(PUBLIC_IDENTITY_RESOURCE_ADDRESS, 0).as_versioned_ref(),
    )
}

pub fn create_genesis_state<TTx>(
    tx: &mut TTx,
    network: Network,
    num_preshards: NumPreshards,
) -> Result<(), StorageError>
where
    TTx: StateStoreWriteTransaction + Deref,
    TTx::Target: StateStoreReadTransaction,
    TTx::Addr: NodeAddressable + Serialize,
{
    if has_bootstrapped(&**tx)? {
        return Ok(());
    }

    let value = Resource::new(
        ResourceType::NonFungible,
        None,
        OwnerRule::None,
        ResourceAccessRules::new(),
        Metadata::from([(TOKEN_SYMBOL, "ID".to_string())]),
        None,
        None,
        0,
        false,
    );
    create_substate(tx, num_preshards, PUBLIC_IDENTITY_RESOURCE_ADDRESS, value)?;

    let symbol = if network.is_testnet() { "tXTR" } else { "XTR" };
    let xtr_resource = Resource::new(
        ResourceType::Stealth,
        None,
        OwnerRule::None,
        ResourceAccessRules::new()
            // These are defaults, but just for explicitness
            .mintable(rule!(deny_all))
            .burnable(rule!(deny_all))
            .recallable(rule!(deny_all))
            .freezable(rule!(deny_all))
            .update_access_rules(rule!(deny_all)),
        Metadata::from([(TOKEN_SYMBOL, symbol)]),
        None,
        None,
        6,
        // Disable total supply tracking for XTR. This is because it is not feasible to include "the fee exhaust" in
        // the tracking (as that would require mutating the resource on every transaction). Tracking supply can
        // be done by summing up the total burn claims (ClaimedOutputTombstone) and subtracting the total exhaust in
        // fee receipts.
        false,
    );

    if network.is_testnet() {
        // Create tXTR faucet
        create_xtr_faucet(tx, num_preshards)?;
        // Create NFT faucet
        create_nft_faucet(tx, num_preshards)?;
    }

    create_substate(tx, num_preshards, STEALTH_TARI_RESOURCE_ADDRESS, xtr_resource)?;

    Ok(())
}

fn create_xtr_faucet<TTx>(tx: &mut TTx, num_preshards: NumPreshards) -> Result<(), StorageError>
where
    TTx: StateStoreWriteTransaction + Deref,
    TTx::Target: StateStoreReadTransaction,
    TTx::Addr: NodeAddressable + Serialize,
{
    let value = Vault::new(ResourceContainer::Stealth {
        address: STEALTH_TARI_RESOURCE_ADDRESS,
        revealed_amount: TXTR_FAUCET_INITIAL_SUPPLY,
        locked_amount: Default::default(),
    });

    create_substate(tx, num_preshards, XTR_FAUCET_VAULT_ADDRESS, value)?;

    let value = ComponentHeader {
        template_address: tari_template_builtin::XTR_FAUCET_TEMPLATE_ADDRESS,
        module_name: "XtrFaucet".to_string(),
        owner_key: None,
        owner_rule: OwnerRule::None,
        access_rules: ComponentAccessRules::allow_all(),
        entity_id: EntityId::default(),
        body: ComponentBody {
            state: cbor!({
                "vault" => XTR_FAUCET_VAULT_ADDRESS,
            })
            .unwrap(),
        },
    };
    create_substate(tx, num_preshards, XTR_FAUCET_COMPONENT_ADDRESS, value)?;

    Ok(())
}

fn create_nft_faucet<TTx>(tx: &mut TTx, num_preshards: NumPreshards) -> Result<(), StorageError>
where
    TTx: StateStoreWriteTransaction + Deref,
    TTx::Target: StateStoreReadTransaction,
    TTx::Addr: NodeAddressable + Serialize,
{
    let value = ComponentHeader {
        template_address: tari_template_builtin::NFT_FAUCET_TEMPLATE_ADDRESS,
        module_name: "NftFaucet".to_string(),
        owner_key: None,
        owner_rule: OwnerRule::None,
        access_rules: ComponentAccessRules::allow_all(),
        entity_id: EntityId::default(),
        body: ComponentBody {
            state: cbor!({"serial_number" => 0u64}).unwrap(),
        },
    };
    create_substate(tx, num_preshards, NFT_FAUCET_COMPONENT_ADDRESS, value)?;

    let metadata = Metadata::from([("name", "NFT Faucet"), (TOKEN_SYMBOL, "tNFT")]);

    let access_rules = ResourceAccessRules::new().mintable(rule!(component(NFT_FAUCET_COMPONENT_ADDRESS)));
    let value = Resource::new(
        ResourceType::NonFungible,
        None,
        OwnerRule::None,
        access_rules,
        metadata,
        None,
        None,
        0,
        true,
    );

    create_substate(tx, num_preshards, NFT_FAUCET_RESOURCE_ADDRESS, value)?;
    Ok(())
}

fn create_substate<TTx, TId, TVal>(
    tx: &mut TTx,
    num_preshards: NumPreshards,
    substate_id: TId,
    value: TVal,
) -> Result<(), StorageError>
where
    TTx: StateStoreWriteTransaction + Deref,
    TTx::Target: StateStoreReadTransaction,
    TTx::Addr: NodeAddressable + Serialize,
    TId: Into<SubstateId>,
    TVal: Into<SubstateValue>,
{
    let substate_id = substate_id.into();
    let shard = VersionedSubstateIdRef::new(&substate_id, 0).to_shard(num_preshards);
    let mut batch = SubstateUpdateBatch::new(Epoch::zero());
    batch
        .with_transition(shard, INITIAL_STATE_VERSION)
        .push(SubstateTransition::Up {
            id: substate_id,
            version: 0,
            substate_or_hash: value.into().into(),
        });

    SubstateRecord::commit_batch(tx, batch)?;

    Ok(())
}
