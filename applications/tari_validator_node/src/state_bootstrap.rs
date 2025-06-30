//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::ops::Deref;

use serde::Serialize;
use tari_bor::cbor;
use tari_common::configuration::Network;
use tari_common_types::types::FixedHash;
use tari_consensus_types::BlockId;
use tari_engine_types::{
    component::{ComponentBody, ComponentHeader},
    resource::Resource,
    resource_container::ResourceContainer,
    substate::{SubstateId, SubstateValue},
    vault::Vault,
};
use tari_ootle_common_types::{
    Epoch,
    NodeAddressable,
    NumPreshards,
    ShardGroup,
    ToSubstateAddress,
    VersionedSubstateId,
};
use tari_ootle_storage::{
    consensus_models::{Block, SubstateRecord},
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
};
use tari_template_lib::{
    auth::{ComponentAccessRules, OwnerRule, ResourceAccessRules},
    constants::{
        CONFIDENTIAL_TARI_RESOURCE_ADDRESS,
        NFT_FAUCET_COMPONENT_ADDRESS,
        NFT_FAUCET_RESOURCE_ADDRESS,
        PUBLIC_IDENTITY_RESOURCE_ADDRESS,
        XTR_FAUCET_COMPONENT_ADDRESS,
        XTR_FAUCET_VAULT_ADDRESS,
    },
    models::{Amount, Metadata},
    prelude::{ResourceType, RistrettoPublicKeyBytes},
    resource::TOKEN_SYMBOL,
    rule,
    types::EntityId,
};

pub fn has_bootstrapped<TTx: StateStoreReadTransaction>(tx: &TTx) -> Result<bool, StorageError> {
    // Assume that if the public identity resource exists, then the rest of the state has been bootstrapped
    SubstateRecord::exists(
        tx,
        VersionedSubstateId::new(PUBLIC_IDENTITY_RESOURCE_ADDRESS, 0).as_ref(),
    )
}

pub fn bootstrap_state<TTx>(
    tx: &mut TTx,
    network: Network,
    num_preshards: NumPreshards,
    sidechain_id: Option<RistrettoPublicKeyBytes>,
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
        false,
    );
    create_substate(
        tx,
        network,
        num_preshards,
        sidechain_id,
        PUBLIC_IDENTITY_RESOURCE_ADDRESS,
        value,
    )?;

    let is_testnet = !matches!(network, Network::MainNet);
    let symbol = if is_testnet { "tXTR" } else { "XTR" };
    let xtr_resource = Resource::new(
        ResourceType::Confidential,
        None,
        OwnerRule::None,
        ResourceAccessRules::new(),
        Metadata::from([(TOKEN_SYMBOL, symbol)]),
        None,
        None,
        false,
    );

    if is_testnet {
        // Create tXTR faucet
        create_xtr_faucet(tx, network, num_preshards, sidechain_id)?;
        // Create NFT faucet
        create_nft_faucet(tx, network, num_preshards, sidechain_id)?;
    }

    create_substate(
        tx,
        network,
        num_preshards,
        sidechain_id,
        CONFIDENTIAL_TARI_RESOURCE_ADDRESS,
        xtr_resource,
    )?;

    Ok(())
}

fn create_xtr_faucet<TTx>(
    tx: &mut TTx,
    network: Network,
    num_preshards: NumPreshards,
    sidechain_id: Option<RistrettoPublicKeyBytes>,
) -> Result<(), StorageError>
where
    TTx: StateStoreWriteTransaction + Deref,
    TTx::Target: StateStoreReadTransaction,
    TTx::Addr: NodeAddressable + Serialize,
{
    let value = ComponentHeader {
        template_address: tari_template_builtin::XTR_FAUCET_TEMPLATE_ADDRESS,
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
        sidechain_id,
        XTR_FAUCET_COMPONENT_ADDRESS,
        value,
    )?;

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
        sidechain_id,
        XTR_FAUCET_VAULT_ADDRESS,
        value,
    )?;
    Ok(())
}

fn create_nft_faucet<TTx>(
    tx: &mut TTx,
    network: Network,
    num_preshards: NumPreshards,
    sidechain_id: Option<RistrettoPublicKeyBytes>,
) -> Result<(), StorageError>
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
    create_substate(
        tx,
        network,
        num_preshards,
        sidechain_id,
        NFT_FAUCET_COMPONENT_ADDRESS,
        value,
    )?;

    let metadata = Metadata::from([("name", "NFT Faucet"), (TOKEN_SYMBOL, "TNFT")]);

    let access_rules = ResourceAccessRules::new().mintable(rule!(component(NFT_FAUCET_COMPONENT_ADDRESS)));
    let value = Resource::new(
        ResourceType::NonFungible,
        None,
        OwnerRule::None,
        access_rules,
        metadata,
        None,
        None,
        true,
    );

    create_substate(
        tx,
        network,
        num_preshards,
        sidechain_id,
        NFT_FAUCET_RESOURCE_ADDRESS,
        value,
    )?;
    Ok(())
}

fn create_substate<TTx, TId, TVal>(
    tx: &mut TTx,
    network: Network,
    num_preshards: NumPreshards,
    sidechain_id: Option<RistrettoPublicKeyBytes>,
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
        FixedHash::zero(),
        ShardGroup::all_shards(num_preshards),
        FixedHash::default(),
        sidechain_id,
    );
    let substate_id = substate_id.into();
    let id = VersionedSubstateId::new(substate_id, 0);
    let shard = id.to_substate_address().to_shard(num_preshards);
    SubstateRecord {
        version: id.version(),
        substate_id: id.into_substate_id(),
        substate_value: Some(value.into()),
        state_hash: Default::default(),
        created_justify: genesis_block.justify().calculate_id(),
        created_block: BlockId::zero(),
        created_by_shard: shard,
        created_at_epoch: Epoch(0),
        destroyed: None,
    }
    .create(tx)?;

    Ok(())
}
