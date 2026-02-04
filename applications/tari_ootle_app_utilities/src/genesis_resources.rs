//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::resource::Resource;
use tari_ootle_common_types::Network;
use tari_template_lib::types::{
    Metadata,
    OwnerRule,
    ResourceAddress,
    ResourceType,
    access_rules::ResourceAccessRules,
    constants::{PUBLIC_IDENTITY_RESOURCE_ADDRESS, STEALTH_TARI_RESOURCE_ADDRESS, TOKEN_SYMBOL},
    rule,
};

pub fn get_public_identity_resource() -> (ResourceAddress, Resource) {
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
    (PUBLIC_IDENTITY_RESOURCE_ADDRESS, value)
}

pub fn get_stealth_tari_resource(network: Network) -> (ResourceAddress, Resource) {
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
    (STEALTH_TARI_RESOURCE_ADDRESS, xtr_resource)
}
