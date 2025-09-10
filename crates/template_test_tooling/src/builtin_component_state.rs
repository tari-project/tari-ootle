//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use tari_bor::cbor;
use tari_crypto::{ristretto::RistrettoPublicKey, tari_utilities::ByteArray};
use tari_engine::state_store::{StateStoreError, StateWriter};
use tari_engine_types::{
    component::{ComponentBody, ComponentHeader},
    id_provider::{IdProvider, ObjectIds},
    resource::Resource,
    resource_container::ResourceContainer,
    substate::{Substate, SubstateId},
    vault::Vault,
};
use tari_template_builtin::NFT_FAUCET_TEMPLATE_ADDRESS;
use tari_template_lib::{
    auth::{ComponentAccessRules, OwnerRule, ResourceAccessRules},
    constants::{
        NFT_FAUCET_COMPONENT_ADDRESS,
        NFT_FAUCET_RESOURCE_ADDRESS,
        PUBLIC_IDENTITY_RESOURCE_ADDRESS,
        STEALTH_TARI_RESOURCE_ADDRESS,
    },
    models::Metadata,
    prelude::{ResourceType, RistrettoPublicKeyBytes, TemplateAddress},
    resource::TOKEN_SYMBOL,
    rule,
    types::{Amount, EntityId, Hash},
};

use crate::{template_test::test_nft_faucet_component, test_faucet_component};

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
                None,
                OwnerRule::None,
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
    metadata.insert(TOKEN_SYMBOL, "tXTR".to_string());
    state_db.set_state(
        id,
        Substate::new(
            0,
            Resource::new(
                ResourceType::Stealth,
                None,
                OwnerRule::None,
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

pub fn initialize_builtin_faucet_state<TStore: StateWriter>(
    store: &mut TStore,
    signer_public_key: &RistrettoPublicKey,
    test_faucet_template_address: TemplateAddress,
) {
    let initial_supply = Amount::from(1_000_000);
    let entity_id = EntityId::default();
    let object_ids = ObjectIds::new(10);
    let id_provider = IdProvider::new(entity_id, Hash::default(), &object_ids);
    let vault_id = id_provider.new_vault_id().unwrap();
    let vault = Vault::new(ResourceContainer::stealth(
        STEALTH_TARI_RESOURCE_ADDRESS,
        initial_supply,
    ));
    store
        .set_state(SubstateId::Vault(vault_id), Substate::new(0, vault))
        .unwrap();

    // This must mirror the test faucet component
    let state = cbor!({
        "vault" => tari_template_lib::models::Vault::for_test(vault_id),
    })
    .unwrap();
    store
        .set_state(
            SubstateId::Component(test_faucet_component()),
            Substate::new(0, ComponentHeader {
                template_address: test_faucet_template_address,
                module_name: "TestFaucet".to_string(),
                owner_key: Some(RistrettoPublicKeyBytes::from_bytes(signer_public_key.as_bytes()).unwrap()),
                owner_rule: OwnerRule::None,
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
        None,
        OwnerRule::None,
        ResourceAccessRules::new().mintable(rule!(component(NFT_FAUCET_COMPONENT_ADDRESS))),
        Default::default(),
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
                owner_key: None,
                owner_rule: OwnerRule::None,
                access_rules: ComponentAccessRules::allow_all(),
                entity_id: EntityId::default(),
                body: ComponentBody { state },
            }),
        )
        .unwrap();
}
