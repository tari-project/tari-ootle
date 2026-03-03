//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use tari_bor::cbor;
use tari_engine::state_store::{StateStoreError, StateWriter};
use tari_engine_types::{
    component::{ComponentBody, ComponentHeader},
    resource::Resource,
    resource_container::ResourceContainer,
    substate::{Substate, SubstateId},
    vault::Vault,
};
use tari_template_builtin::{NFT_FAUCET_TEMPLATE_ADDRESS, XTR_FAUCET_TEMPLATE_ADDRESS};
use tari_template_lib::types::{
    Amount,
    EntityId,
    Metadata,
    ResourceType,
    access_rules::{ComponentAccessRules, ResourceAccessRules},
    constants::{
        NFT_FAUCET_COMPONENT_ADDRESS,
        NFT_FAUCET_RESOURCE_ADDRESS,
        PUBLIC_IDENTITY_RESOURCE_ADDRESS,
        STEALTH_TARI_RESOURCE_ADDRESS,
        TOKEN_SYMBOL,
        XTR_FAUCET_VAULT_ADDRESS,
    },
    metadata,
    rule,
};

use crate::{template_lib_types::SubstateOwnerRule, template_test::test_nft_faucet_component, xtr_faucet_component};

pub fn add_tari_resources<T: StateWriter>(state_db: &mut T) -> Result<(), StateStoreError> {
    let id = SubstateId::Resource(PUBLIC_IDENTITY_RESOURCE_ADDRESS);
    let mut metadata = Metadata::new();
    metadata.insert(TOKEN_SYMBOL, "ID".to_string());
    // Create the resource for badges
    state_db.set_state(
        id,
        Substate::new(
            0,
            Resource::new(
                ResourceType::NonFungible,
                SubstateOwnerRule::None,
                ResourceAccessRules::deny_all(),
                metadata,
                None,
                None,
                0,
                false,
            ),
        ),
    )?;

    // Create the second layer tari resource
    let id = SubstateId::Resource(STEALTH_TARI_RESOURCE_ADDRESS);
    let mut metadata = Metadata::new();
    metadata.insert(TOKEN_SYMBOL, "tTARI".to_string());
    state_db.set_state(
        id,
        Substate::new(
            0,
            Resource::new(
                ResourceType::Stealth,
                SubstateOwnerRule::None,
                ResourceAccessRules::new(),
                metadata,
                None,
                None,
                6,
                true,
            ),
        ),
    )?;

    Ok(())
}

pub fn initialize_builtin_faucet_state<TStore: StateWriter>(store: &mut TStore) {
    let initial_supply = Amount::MAX;
    let entity_id = EntityId::default();
    let vault = Vault::new(ResourceContainer::stealth(
        STEALTH_TARI_RESOURCE_ADDRESS,
        initial_supply,
    ));
    store
        .set_state(SubstateId::Vault(XTR_FAUCET_VAULT_ADDRESS), Substate::new(0, vault))
        .unwrap();

    // This must mirror the test faucet component
    let state = cbor!({
        "vault" => tari_template_lib::models::Vault::for_test(XTR_FAUCET_VAULT_ADDRESS),
    })
    .unwrap();
    store
        .set_state(
            SubstateId::Component(xtr_faucet_component()),
            Substate::new(0, ComponentHeader {
                template_address: XTR_FAUCET_TEMPLATE_ADDRESS,
                module_name: "XtrFaucet".to_string(),
                owner_rule: SubstateOwnerRule::None,
                access_rules: ComponentAccessRules::allow_all(),
                entity_id,
                body: ComponentBody { state },
            }),
        )
        .unwrap();
}

pub fn initialize_builtin_nft_faucet_state<TStore: StateWriter>(store: &mut TStore) {
    let resource = Resource::new(
        ResourceType::NonFungible,
        SubstateOwnerRule::None,
        ResourceAccessRules::new().mintable(rule!(component(NFT_FAUCET_COMPONENT_ADDRESS))),
        metadata!(TOKEN_SYMBOL => "tNFT"),
        None,
        None,
        0,
        true,
    );

    store
        .set_state(
            SubstateId::Resource(NFT_FAUCET_RESOURCE_ADDRESS),
            Substate::new(0, resource),
        )
        .unwrap();

    let state = cbor!({
        "serial_number" => 0u64,
    })
    .unwrap();
    store
        .set_state(
            SubstateId::Component(test_nft_faucet_component()),
            Substate::new(0, ComponentHeader {
                template_address: NFT_FAUCET_TEMPLATE_ADDRESS,
                module_name: "TestFaucet".to_string(),
                owner_rule: SubstateOwnerRule::None,
                access_rules: ComponentAccessRules::allow_all(),
                entity_id: EntityId::default(),
                body: ComponentBody { state },
            }),
        )
        .unwrap();
}
