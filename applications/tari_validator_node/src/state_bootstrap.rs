//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::ops::Deref;

use serde::Serialize;
use tari_bor::cbor;
use tari_common::configuration::Network;
use tari_common_types::types::FixedHash;
use tari_crypto::ristretto::RistrettoPublicKey;
use tari_dan_common_types::{
    shard::Shard,
    Epoch,
    NodeAddressable,
    NodeHeight,
    NumPreshards,
    ShardGroup,
    VersionedSubstateId,
};
use tari_dan_storage::{
    consensus_models::{Block, BlockId, SubstateRecord},
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
};
use tari_engine_types::{
    component::{ComponentBody, ComponentHeader},
    resource::Resource,
    resource_container::ResourceContainer,
    substate::{SubstateId, SubstateValue},
    vault::Vault,
};
use tari_template_lib::{
    auth::{ComponentAccessRules, OwnerRule, ResourceAccessRules},
    constants::{
        CONFIDENTIAL_TARI_RESOURCE_ADDRESS,
        PUBLIC_IDENTITY_RESOURCE_ADDRESS,
        XTR_FAUCET_COMPONENT_ADDRESS,
        XTR_FAUCET_VAULT_ADDRESS,
    },
    models::{Amount, EntityId, Metadata},
    prelude::ResourceType,
    resource::TOKEN_SYMBOL,
};

pub fn has_bootstrapped<TTx: StateStoreReadTransaction>(tx: &TTx) -> Result<bool, StorageError> {
    // Assume that if the public identity resource exists, then the rest of the state has been bootstrapped
    SubstateRecord::exists(tx, &VersionedSubstateId::new(PUBLIC_IDENTITY_RESOURCE_ADDRESS, 0))
}

pub fn bootstrap_state<TTx>(
    tx: &mut TTx,
    network: Network,
    num_preshards: NumPreshards,
    sidechain_id: Option<RistrettoPublicKey>,
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
    );
    create_substate(
        tx,
        network,
        num_preshards,
        &sidechain_id,
        PUBLIC_IDENTITY_RESOURCE_ADDRESS,
        value,
    )?;

    let mut xtr_resource = Resource::new(
        ResourceType::Confidential,
        None,
        OwnerRule::None,
        ResourceAccessRules::new(),
        Metadata::from([(TOKEN_SYMBOL, "XTR".to_string())]),
        None,
        None,
    );

    // Create faucet component
    if !matches!(network, Network::MainNet) {
        let value = ComponentHeader {
            template_address: tari_template_builtin::FAUCET_TEMPLATE_ADDRESS,
            module_name: "XtrFaucet".to_string(),
            owner_key: None,
            owner_rule: OwnerRule::None,
            access_rules: ComponentAccessRules::allow_all(),
            entity_id: EntityId::default(),
            body: ComponentBody {
                state: cbor!({"vault" => XTR_FAUCET_VAULT_ADDRESS}).unwrap(),
            },
        };
        create_substate(
            tx,
            network,
            num_preshards,
            &sidechain_id,
            XTR_FAUCET_COMPONENT_ADDRESS,
            value,
        )?;

        xtr_resource.increase_total_supply(Amount::MAX);
        let value = Vault::new(ResourceContainer::Confidential {
            address: CONFIDENTIAL_TARI_RESOURCE_ADDRESS,
            commitments: Default::default(),
            revealed_amount: Amount::MAX,
            locked_commitments: Default::default(),
            locked_revealed_amount: Default::default(),
        });

        create_substate(
            tx,
            network,
            num_preshards,
            &sidechain_id,
            XTR_FAUCET_VAULT_ADDRESS,
            value,
        )?;
    }

    create_substate(
        tx,
        network,
        num_preshards,
        &sidechain_id,
        CONFIDENTIAL_TARI_RESOURCE_ADDRESS,
        xtr_resource,
    )?;

    Ok(())
}

fn create_substate<TTx, TId, TVal>(
    tx: &mut TTx,
    network: Network,
    num_preshards: NumPreshards,
    sidechain_id: &Option<RistrettoPublicKey>,
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
    let genesis_block = Block::genesis(
        network,
        Epoch(0),
        ShardGroup::all_shards(num_preshards),
        FixedHash::default(),
        sidechain_id.clone(),
    );
    let substate_id = substate_id.into();
    let id = VersionedSubstateId::new(substate_id, 0);
    SubstateRecord {
        version: id.version(),
        substate_id: id.into_substate_id(),
        substate_value: Some(value.into()),
        state_hash: Default::default(),
        created_by_transaction: Default::default(),
        created_justify: *genesis_block.justify().id(),
        created_block: BlockId::zero(),
        created_height: NodeHeight(0),
        created_by_shard: Shard::zero(),
        created_at_epoch: Epoch(0),
        destroyed: None,
    }
    .create(tx)?;
    Ok(())
}
