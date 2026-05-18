//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_bor::cbor;
use tari_engine::state_store::{StateWriter, memory::MemoryStateStore};
use tari_engine_types::{
    component::{Component, ComponentBody, ComponentHeader},
    resource::Resource,
    resource_container::ResourceContainer,
    substate::Substate,
    vault::Vault,
};
use tari_template_builtin::XTR_FAUCET_TEMPLATE_ADDRESS;
use tari_template_lib::types::{
    Amount,
    ComponentAddress,
    ObjectKey,
    ResourceType,
    SubstateOwnerRule,
    VaultId,
    access_rules::{ComponentAccessRules, ResourceAccessRules},
    constants::{TARI_TOKEN, XTR_FAUCET_CLAIM_RESOURCE_ADDRESS},
    metadata,
    rule,
};

pub const FAUCET_COMPONENT_ADDRESS: ComponentAddress = ComponentAddress::from_array([0u8; 32]);

pub const FAUCET_VAULT_ID: VaultId = VaultId::new(ObjectKey::from_array([1u8; 32]));

pub fn setup_store() -> MemoryStateStore {
    let mut state_store = MemoryStateStore::new();
    let tari = Resource::new(
        ResourceType::Stealth,
        SubstateOwnerRule::None,
        ResourceAccessRules::new(),
        metadata!(),
        None,
        None,
        6,
        true,
    );
    state_store
        .set_state(TARI_TOKEN.into(), Substate::new(0, tari))
        .unwrap();

    let resource_cont = ResourceContainer::Stealth {
        address: TARI_TOKEN,
        revealed_amount: Amount::MAX,
        locked_amount: Amount::zero(),
    };
    let vault = Vault::new(resource_cont);

    state_store
        .set_state(FAUCET_VAULT_ID.into(), Substate::new(0, vault))
        .unwrap();

    // Claim receipt resource: one NFT per claimant public key (minted then burned to record the claim).
    let claim_resource = Resource::new(
        ResourceType::NonFungible,
        SubstateOwnerRule::None,
        ResourceAccessRules::new()
            .mintable(rule!(component(FAUCET_COMPONENT_ADDRESS)))
            .burnable(rule!(allow_all)),
        Default::default(),
        None,
        None,
        0,
        false,
    );
    state_store
        .set_state(
            XTR_FAUCET_CLAIM_RESOURCE_ADDRESS.into(),
            Substate::new(0, claim_resource),
        )
        .unwrap();

    let component = Component {
        header: ComponentHeader {
            template_address: XTR_FAUCET_TEMPLATE_ADDRESS,
            owner_rule: SubstateOwnerRule::None,
            access_rules: ComponentAccessRules::allow_all(),
            entity_id: Default::default(),
        },
        body: ComponentBody::from_cbor_value(
            cbor!({
                "vault" => FAUCET_VAULT_ID,
            })
            .unwrap(),
        ),
    };
    state_store
        .set_state(FAUCET_COMPONENT_ADDRESS.into(), Substate::new(0, component))
        .unwrap();

    state_store
}
