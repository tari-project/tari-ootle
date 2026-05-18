//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::ops::Deref;

use serde::Serialize;
use tari_bor::cbor;
use tari_engine_types::{
    component::{Component, ComponentBody, ComponentHeader},
    resource::Resource,
    resource_container::ResourceContainer,
    substate::{SubstateId, SubstateValue},
    vault::Vault,
};
use tari_ootle_app_utilities::{
    genesis_resources::{get_public_identity_resource, get_stealth_tari_resource},
    shared_consts::TXTR_FAUCET_INITIAL_SUPPLY,
};
use tari_ootle_common_types::{Epoch, NodeAddressable, NumPreshards, VersionedSubstateId, VersionedSubstateIdRef};
use tari_ootle_storage::{
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
    consensus_models::{SubstateRecord, SubstateTransition, SubstateUpdateBatch},
};
use tari_ootle_transaction::Network;
use tari_template_lib::types::{
    EntityId,
    Metadata,
    ResourceType,
    SubstateOwnerRule,
    access_rules::{ComponentAccessRules, LOCKED, ResourceAccessRules},
    constants::{
        NFT_FAUCET_COMPONENT_ADDRESS,
        NFT_FAUCET_RESOURCE_ADDRESS,
        PUBLIC_IDENTITY_RESOURCE_ADDRESS,
        STEALTH_TARI_RESOURCE_ADDRESS,
        TOKEN_SYMBOL,
        XTR_FAUCET_CLAIM_RESOURCE_ADDRESS,
        XTR_FAUCET_COMPONENT_ADDRESS,
        XTR_FAUCET_VAULT_ADDRESS,
    },
    rule,
};

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

    let (public_identity_address, resource) = get_public_identity_resource();
    create_substate(tx, num_preshards, public_identity_address, resource)?;

    let (xtr_address, xtr_resource) = get_stealth_tari_resource(network);
    create_substate(tx, num_preshards, xtr_address, xtr_resource)?;

    if network.is_testnet() {
        // Create tXTR faucet
        create_xtr_faucet(tx, num_preshards)?;
        // Create NFT faucet
        create_nft_faucet(tx, num_preshards)?;
    }

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

    let value = Component {
        header: ComponentHeader {
            template_address: tari_template_builtin::XTR_FAUCET_TEMPLATE_ADDRESS,
            owner_rule: SubstateOwnerRule::None,
            access_rules: ComponentAccessRules::allow_all(),
            entity_id: EntityId::default(),
        },
        body: ComponentBody::from_cbor_value(
            cbor!({
                "vault" => XTR_FAUCET_VAULT_ADDRESS,
            })
            .unwrap(),
        ),
    };
    create_substate(tx, num_preshards, XTR_FAUCET_COMPONENT_ADDRESS, value)?;

    // Create the claim receipt resource: one NFT per claimant public key, immediately burned after minting.
    // The burned substate key persists on-chain, preventing duplicate claims.
    let claim_resource = Resource::new(
        ResourceType::NonFungible,
        SubstateOwnerRule::None,
        ResourceAccessRules::new()
            .mintable(rule!(component(XTR_FAUCET_COMPONENT_ADDRESS)), LOCKED)
            .burnable(rule!(component(XTR_FAUCET_COMPONENT_ADDRESS)), LOCKED),
        Metadata::new(),
        None,
        None,
        0,
        false,
    );
    create_substate(tx, num_preshards, XTR_FAUCET_CLAIM_RESOURCE_ADDRESS, claim_resource)?;

    Ok(())
}

fn create_nft_faucet<TTx>(tx: &mut TTx, num_preshards: NumPreshards) -> Result<(), StorageError>
where
    TTx: StateStoreWriteTransaction + Deref,
    TTx::Target: StateStoreReadTransaction,
    TTx::Addr: NodeAddressable + Serialize,
{
    let value = Component {
        header: ComponentHeader {
            template_address: tari_template_builtin::NFT_FAUCET_TEMPLATE_ADDRESS,
            owner_rule: SubstateOwnerRule::None,
            access_rules: ComponentAccessRules::allow_all(),
            entity_id: EntityId::default(),
        },
        body: ComponentBody {
            state: cbor!({"serial_number" => 0u64}).unwrap(),
        },
    };
    create_substate(tx, num_preshards, NFT_FAUCET_COMPONENT_ADDRESS, value)?;

    let metadata = Metadata::from([("name", "NFT Faucet"), (TOKEN_SYMBOL, "tNFT")]);

    let access_rules = ResourceAccessRules::new().mintable(rule!(component(NFT_FAUCET_COMPONENT_ADDRESS)), LOCKED);
    let value = Resource::new(
        ResourceType::NonFungible,
        SubstateOwnerRule::None,
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
    batch.with_transition(shard, 0).push(SubstateTransition::Up {
        id: substate_id,
        version: 0,
        substate_or_hash: value.into().into(),
    });

    SubstateRecord::commit_batch(tx, batch)?;

    Ok(())
}
