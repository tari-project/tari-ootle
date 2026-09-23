//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use tari_engine::state_store::{StateStoreError, StateWriter};
use tari_engine_types::{
    component::{Component, ComponentBody, ComponentHeader},
    resource::Resource,
    resource_container::ResourceContainer,
    substate::{Substate, SubstateId},
    vault::Vault,
};
use tari_template_builtin::{
    BURN_RATE_GOVERNANCE_TEMPLATE_ADDRESS,
    NFT_FAUCET_TEMPLATE_ADDRESS,
    NftFaucetState,
    XTR_FAUCET_TEMPLATE_ADDRESS,
    XtrFaucetState,
};
use tari_template_lib::types::{
    Amount,
    EntityId,
    Metadata,
    ResourceType,
    access_rules::{ComponentAccessRules, LOCKED, ResourceAccessRules},
    constants::{
        BURN_RATE_GOVERNANCE_COMPONENT_ADDRESS,
        NFT_FAUCET_COMPONENT_ADDRESS,
        NFT_FAUCET_RESOURCE_ADDRESS,
        PUBLIC_IDENTITY_RESOURCE_ADDRESS,
        STEALTH_TARI_RESOURCE_ADDRESS,
        TOKEN_SYMBOL,
        XTR_FAUCET_CLAIM_RESOURCE_ADDRESS,
        XTR_FAUCET_VAULT_ADDRESS,
    },
    crypto::RistrettoPublicKeyBytes,
    governance::{BurnRateGovernanceState, council_owner_rule, no_council_owner_rule},
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

    let state = tari_bor::to_value(&XtrFaucetState {
        vault: XTR_FAUCET_VAULT_ADDRESS,
    })
    .expect("XtrFaucetState encode is infallible");
    store
        .set_state(
            SubstateId::Component(xtr_faucet_component()),
            Substate::new(0, Component {
                header: ComponentHeader {
                    template_address: XTR_FAUCET_TEMPLATE_ADDRESS,
                    owner_rule: SubstateOwnerRule::None,
                    access_rules: ComponentAccessRules::allow_all(),
                    entity_id,
                },
                body: ComponentBody { state },
            }),
        )
        .unwrap();

    // Claim receipt resource: one NFT per claimant public key (minted then burned to record the claim).
    let claim_resource = Resource::new(
        ResourceType::NonFungible,
        SubstateOwnerRule::None,
        ResourceAccessRules::new()
            .mintable(rule!(component(xtr_faucet_component())), LOCKED)
            .burnable(rule!(allow_all), LOCKED),
        Metadata::new(),
        None,
        None,
        0,
        false,
    );
    store
        .set_state(
            SubstateId::Resource(XTR_FAUCET_CLAIM_RESOURCE_ADDRESS),
            Substate::new(0, claim_resource),
        )
        .unwrap();
}

pub fn initialize_builtin_nft_faucet_state<TStore: StateWriter>(store: &mut TStore) {
    let resource = Resource::new(
        ResourceType::NonFungible,
        SubstateOwnerRule::None,
        ResourceAccessRules::new().mintable(rule!(component(NFT_FAUCET_COMPONENT_ADDRESS)), LOCKED),
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

    let state = tari_bor::to_value(&NftFaucetState { serial_number: 0 }).expect("NftFaucetState encode is infallible");
    store
        .set_state(
            SubstateId::Component(test_nft_faucet_component()),
            Substate::new(0, Component {
                header: ComponentHeader {
                    template_address: NFT_FAUCET_TEMPLATE_ADDRESS,
                    owner_rule: SubstateOwnerRule::None,
                    access_rules: ComponentAccessRules::allow_all(),
                    entity_id: EntityId::default(),
                },
                body: ComponentBody { state },
            }),
        )
        .unwrap();
}

/// Creates the burn rate governance component with `council` owning it at `threshold`.
///
/// Genesis creates this on every network, so a test that calls a council method needs it the same
/// way a network does. A threshold of zero leaves it owned by nobody, which is what a network that
/// seats no council gets.
pub fn initialize_burn_rate_governance_state<TStore: StateWriter>(
    store: &mut TStore,
    threshold: u16,
    council: &[RistrettoPublicKeyBytes],
) {
    let owner_rule = if threshold == 0 {
        no_council_owner_rule()
    } else {
        council_owner_rule(threshold, council)
    };

    let state =
        tari_bor::to_value(&BurnRateGovernanceState::new()).expect("BurnRateGovernanceState encode is infallible");
    store
        .set_state(
            SubstateId::Component(BURN_RATE_GOVERNANCE_COMPONENT_ADDRESS),
            Substate::new(0, Component {
                header: ComponentHeader {
                    template_address: BURN_RATE_GOVERNANCE_TEMPLATE_ADDRESS,
                    owner_rule,
                    access_rules: ComponentAccessRules::new(),
                    entity_id: EntityId::default(),
                },
                body: ComponentBody { state },
            }),
        )
        .unwrap();
}
