//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, ops::Deref};

use serde::Serialize;
use tari_engine_types::{
    component::{Component, ComponentBody, ComponentHeader},
    resource::Resource,
    resource_container::ResourceContainer,
    substate::{SubstateId, SubstateValue, hash_substate},
    vault::Vault,
};
use tari_ootle_app_utilities::{
    genesis_resources::{get_public_identity_resource, get_stealth_tari_resource},
    shared_consts::TXTR_FAUCET_INITIAL_SUPPLY,
};
use tari_ootle_common_types::{
    Epoch,
    NodeAddressable,
    NumPreshards,
    VersionedSubstateId,
    VersionedSubstateIdRef,
    shard::Shard,
};
use tari_ootle_storage::{
    ShardScopedTreeStoreWriter,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
    consensus_models::{SubstateRecord, SubstateTransition, SubstateUpdateBatch},
};
use tari_ootle_transaction::Network;
use tari_state_tree::{SpreadPrefixStateTree, SubstateTreeChange, Version};
use tari_template_builtin::{NftFaucetState, XtrFaucetState};
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

/// The state version (and epoch) that bootstrapped genesis state is committed at. Consensus and
/// state-sync state changes begin at version 1 (see the state-sync `start_state_version` logic).
const GENESIS_STATE_VERSION: Version = 0;

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

    let mut substates: Vec<(SubstateId, SubstateValue)> = Vec::new();

    let (public_identity_address, resource) = get_public_identity_resource();
    substates.push((public_identity_address.into(), resource.into()));

    let (xtr_address, xtr_resource) = get_stealth_tari_resource(network);
    substates.push((xtr_address.into(), xtr_resource.into()));

    if network.is_testnet() {
        // Create tXTR faucet
        substates.extend(xtr_faucet_substates());
        // Create NFT faucet
        substates.extend(nft_faucet_substates());
    }

    commit_genesis_substates(tx, num_preshards, substates)?;

    Ok(())
}

fn xtr_faucet_substates() -> Vec<(SubstateId, SubstateValue)> {
    let vault = Vault::new(ResourceContainer::Stealth {
        address: STEALTH_TARI_RESOURCE_ADDRESS,
        revealed_amount: TXTR_FAUCET_INITIAL_SUPPLY,
        locked_amount: Default::default(),
    });

    let component = Component {
        header: ComponentHeader {
            template_address: tari_template_builtin::XTR_FAUCET_TEMPLATE_ADDRESS,
            owner_rule: SubstateOwnerRule::None,
            access_rules: ComponentAccessRules::allow_all(),
            entity_id: EntityId::default(),
        },
        body: ComponentBody::from_cbor_value(
            tari_bor::to_value(&XtrFaucetState {
                vault: XTR_FAUCET_VAULT_ADDRESS,
            })
            .expect("XtrFaucetState encode is infallible"),
        ),
    };

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

    vec![
        (XTR_FAUCET_VAULT_ADDRESS.into(), vault.into()),
        (XTR_FAUCET_COMPONENT_ADDRESS.into(), component.into()),
        (XTR_FAUCET_CLAIM_RESOURCE_ADDRESS.into(), claim_resource.into()),
    ]
}

fn nft_faucet_substates() -> Vec<(SubstateId, SubstateValue)> {
    let component = Component {
        header: ComponentHeader {
            template_address: tari_template_builtin::NFT_FAUCET_TEMPLATE_ADDRESS,
            owner_rule: SubstateOwnerRule::None,
            access_rules: ComponentAccessRules::allow_all(),
            entity_id: EntityId::default(),
        },
        body: ComponentBody::from_cbor_value(
            tari_bor::to_value(&NftFaucetState { serial_number: 0 }).expect("NftFaucetState encode is infallible"),
        ),
    };

    let metadata = Metadata::from([("name", "NFT Faucet"), (TOKEN_SYMBOL, "tNFT")]);
    let access_rules = ResourceAccessRules::new().mintable(rule!(component(NFT_FAUCET_COMPONENT_ADDRESS)), LOCKED);
    let resource = Resource::new(
        ResourceType::NonFungible,
        SubstateOwnerRule::None,
        access_rules,
        metadata,
        None,
        None,
        0,
        true,
    );

    vec![
        (NFT_FAUCET_COMPONENT_ADDRESS.into(), component.into()),
        (NFT_FAUCET_RESOURCE_ADDRESS.into(), resource.into()),
    ]
}

/// Commits the genesis substates to both the substate store and the per-shard state tree (JMT) at
/// [`GENESIS_STATE_VERSION`].
///
/// Writing them to the state tree (not just the substate store) is what allows them to be
/// cryptographically proven on read. Without a tree leaf, immutable genesis substates such as the
/// TARI resource have no inclusion proof and verified reads fail with a leaf-key mismatch.
fn commit_genesis_substates<TTx>(
    tx: &mut TTx,
    num_preshards: NumPreshards,
    substates: Vec<(SubstateId, SubstateValue)>,
) -> Result<(), StorageError>
where
    TTx: StateStoreWriteTransaction + Deref,
    TTx::Target: StateStoreReadTransaction,
{
    let mut batch = SubstateUpdateBatch::new(Epoch::zero());
    let mut tree_changes: HashMap<Shard, Vec<SubstateTreeChange>> = HashMap::new();

    for (substate_id, value) in substates {
        let shard = VersionedSubstateIdRef::new(&substate_id, 0).to_shard(num_preshards);
        let value_hash = hash_substate(&value, 0, Epoch::zero());

        batch
            .with_transition(shard, GENESIS_STATE_VERSION)
            .push(SubstateTransition::Up {
                id: substate_id.clone(),
                version: 0,
                substate_or_hash: value.into(),
            });

        tree_changes.entry(shard).or_default().push(SubstateTreeChange::Up {
            id: VersionedSubstateId::new(substate_id, 0),
            value_hash,
        });
    }

    SubstateRecord::commit_batch(tx, batch)?;

    // Commit the genesis leaves into each shard's state tree at version 0. Consensus then builds
    // version 1 onwards on top of this (next_version = current_version.unwrap_or(0) + 1).
    for (shard, changes) in tree_changes {
        let mut store = ShardScopedTreeStoreWriter::new(tx, shard);
        SpreadPrefixStateTree::new(&mut store)
            .batch_put_substate_changes(None, GENESIS_STATE_VERSION, changes)
            .map_err(|e| StorageError::QueryError {
                reason: format!("commit_genesis_substates: failed to write state tree for {shard}: {e}"),
            })?;
        store
            .set_state_version(GENESIS_STATE_VERSION)
            .map_err(|e| StorageError::QueryError {
                reason: format!("commit_genesis_substates: failed to set state version for {shard}: {e}"),
            })?;
    }

    Ok(())
}
