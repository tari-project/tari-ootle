//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! What a transaction can do to component code, for whoever signs or approves it.
//!
//! Two different things are reported:
//!
//! - [`component_upgrades`]: the `UpdateComponentTemplate` instructions in the transaction itself. Upgrades exist only
//!   as transaction instructions, so reading the instruction list finds every upgrade the transaction can perform. A
//!   signer who satisfies a component's upgrade rule hands that component's code, and every vault it holds, to the new
//!   template.
//! - [`resolve_upgradable_components`]: components the transaction uses whose code someone else may replace later.

use std::collections::{HashMap, HashSet};

use axum_extra::headers::authorization::Bearer;
use indexmap::IndexSet;
use mini_moka::sync::Cache;
use tari_ootle_common_types::engine_types::{
    component::Component,
    substate::{SubstateDiff, SubstateId},
};
use tari_ootle_transaction::{ComponentReference, Instruction};
use tari_ootle_walletd_client::{
    permissions::Permission,
    types::{ComponentUpgrade, UpgradableComponent},
};
use tari_template_lib_types::{
    ComponentAddress,
    SubstateOwnerRule,
    access_rules::{AccessRule, UpdateRule},
};

use super::{auth::jwt::AuthError, context::HandlerContext};
use crate::WalletSdk;

/// Upper bound on the components whose upgrade rule the daemon keeps in memory.
pub const UPGRADE_RULE_CACHE_MAX_ENTRIES: u64 = 10_000;

/// Every `UpdateComponentTemplate` instruction in the transaction, fee instructions first.
pub fn component_upgrades(fee_instructions: &[Instruction], instructions: &[Instruction]) -> Vec<ComponentUpgrade> {
    fee_instructions
        .iter()
        .chain(instructions)
        .filter_map(|instruction| match instruction {
            Instruction::UpdateComponentTemplate {
                component,
                migrate,
                new_template,
            } => Some(ComponentUpgrade {
                component: component.clone(),
                new_template: *new_template,
                migrate: migrate.clone(),
            }),
            _ => None,
        })
        .collect()
}

/// Refuses a submission that upgrades a component unless the caller holds `admin`.
///
/// `transactions:create` lets a tool have the wallet sign arbitrary transactions with no person reviewing them. An
/// upgrade signed by a key that satisfies the component's upgrade rule gives the new template everything the
/// component holds, so it needs the credential a person keeps for themselves. Transaction and signing requests are
/// gated by a person's approval instead, made after seeing [`component_upgrades`].
pub fn authorize_component_upgrades(
    context: &HandlerContext,
    token: Option<&Bearer>,
    upgrades: &[ComponentUpgrade],
) -> Result<(), AuthError> {
    let Some(upgrade) = upgrades.first() else {
        return Ok(());
    };
    match context.authorize(token, &[Permission::Admin]) {
        Ok(_) => Ok(()),
        Err(AuthError::InsufficientPermissions { .. }) => Err(AuthError::ComponentUpgradeRequiresAdmin {
            component: display_component_reference(&upgrade.component),
            new_template: upgrade.new_template.to_string(),
        }),
        Err(err) => Err(err),
    }
}

fn display_component_reference(component: &ComponentReference) -> String {
    match component {
        ComponentReference::Address(address) => address.to_string(),
        ComponentReference::Workspace(id) => format!("in workspace slot {id}"),
    }
}

/// Upgrade rules of components this daemon has read.
///
/// Every entry stays valid: a component's upgrade rule is fixed when the component is created. The one rule derived
/// from mutable state, an ownerless `Owner` rule stored as [`UpdateRule::Locked`], is final too, see
/// [`effective_upgrade_rule`].
#[derive(Clone)]
pub struct UpgradeRuleCache {
    rules: Cache<ComponentAddress, UpdateRule>,
}

impl std::fmt::Debug for UpgradeRuleCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpgradeRuleCache")
            .field("entries", &self.rules.entry_count())
            .finish()
    }
}

impl UpgradeRuleCache {
    pub fn new(max_entries: u64) -> Self {
        Self {
            rules: Cache::builder().max_capacity(max_entries).build(),
        }
    }

    fn get(&self, address: &ComponentAddress) -> Option<UpdateRule> {
        self.rules.get(address)
    }

    fn insert(&self, address: ComponentAddress, component: &Component) {
        self.rules.insert(address, effective_upgrade_rule(component));
    }
}

/// The component's upgrade rule, with an `Owner` rule on an ownerless component reported as [`UpdateRule::Locked`].
///
/// [`SubstateOwnerRule::None`] satisfies no caller, and replacing an owner rule requires satisfying it, so an
/// ownerless component stays ownerless and its `Owner` upgrade rule can never pass.
fn effective_upgrade_rule(component: &Component) -> UpdateRule {
    match component.upgrade_rule() {
        UpdateRule::Owner if matches!(component.owner_rule(), SubstateOwnerRule::None) => UpdateRule::Locked,
        rule => rule.clone(),
    }
}

/// Upgrade rules the cache holds for the directly called components, and the components it lacks, in instruction
/// order without duplicates.
///
/// The cache may evict an entry at any time, so the rules are read out once here and resolution works from the
/// returned map.
pub fn cached_upgrade_rules(
    cache: &UpgradeRuleCache,
    called: &[ComponentAddress],
) -> (HashMap<ComponentAddress, UpdateRule>, Vec<ComponentAddress>) {
    let mut known = HashMap::new();
    let mut uncached = Vec::new();
    for address in called.iter().copied().collect::<IndexSet<_>>() {
        match cache.get(&address) {
            Some(rule) => {
                known.insert(address, rule);
            },
            None => uncached.push(address),
        }
    }
    (known, uncached)
}

/// Fetches each of `addresses` from the network and returns its upgrade rule, caching it for later transactions. A
/// component that does not exist yet is absent: nothing can upgrade it before this transaction creates it.
pub async fn fetch_upgrade_rules(
    sdk: &WalletSdk,
    cache: &UpgradeRuleCache,
    addresses: &[ComponentAddress],
) -> Result<HashMap<ComponentAddress, UpdateRule>, anyhow::Error> {
    let ids = addresses.iter().copied().map(SubstateId::Component).collect::<Vec<_>>();
    let fetched = sdk.substate_api().get_substates_from_network(&ids).await?;
    let mut rules = HashMap::with_capacity(fetched.len());
    for (id, substate) in fetched {
        if let (Some(address), Some(component)) = (id.as_component_address(), substate.substate_value().component()) {
            cache.insert(address, component);
            rules.insert(address, effective_upgrade_rule(component));
        }
    }
    Ok(rules)
}

/// Components whose template can still be swapped: direct calls in instruction order, then components the dry run
/// wrote that the instructions did not name.
///
/// A component the run wrote takes its rule from the accept diff, whose up'd header carries it, and the cache learns
/// it. A directly called component the run only read takes its rule from `known`, which the caller builds from
/// [`cached_upgrade_rules`] and [`fetch_upgrade_rules`]. A component a nested call only reads is absent: nothing in
/// the diff names it. A component the transaction creates has no down entry and is skipped.
pub fn resolve_upgradable_components(
    cache: &UpgradeRuleCache,
    called: &[ComponentAddress],
    known: &HashMap<ComponentAddress, UpdateRule>,
    diff: Option<&SubstateDiff>,
) -> Vec<UpgradableComponent> {
    let written = diff.map(written_components).unwrap_or_default();
    for (address, component) in &written {
        cache.insert(*address, component);
    }
    let written_by_address = written
        .iter()
        .map(|(address, component)| (*address, effective_upgrade_rule(component)))
        .collect::<HashMap<_, _>>();

    let mut rules = Vec::new();
    let mut seen = IndexSet::new();
    for address in called {
        if !seen.insert(*address) {
            continue;
        }
        if let Some(rule) = written_by_address.get(address).or_else(|| known.get(address)) {
            rules.push((*address, rule.clone()));
        }
    }
    for (address, _) in &written {
        if seen.insert(*address) {
            rules.push((*address, written_by_address[address].clone()));
        }
    }
    filter_upgradable(rules)
}

/// Components the dry run replaced, in diff order.
///
/// A create is an up with no down: the component does not exist until the transaction is accepted, so nothing can
/// swap its template before this submission.
fn written_components(diff: &SubstateDiff) -> Vec<(ComponentAddress, &Component)> {
    let downed = diff.down_iter().map(|(id, _)| id).collect::<HashSet<_>>();
    diff.up_iter()
        .filter(|(id, _)| downed.contains(id))
        .filter_map(|(id, substate)| Some((id.as_component_address()?, substate.substate_value().component()?)))
        .collect()
}

/// Keeps every component whose template can still be swapped, in the given order.
///
/// [`UpdateRule::Locked`] and [`AccessRule::DenyAll`] can never pass `check_component_upgrade_rule`, and the upgrade
/// rule does not grant the owner a bypass.
fn filter_upgradable<I: IntoIterator<Item = (ComponentAddress, UpdateRule)>>(rules: I) -> Vec<UpgradableComponent> {
    rules
        .into_iter()
        .filter(|(_, rule)| !matches!(rule, UpdateRule::Locked | UpdateRule::AccessRule(AccessRule::DenyAll)))
        .map(|(component_address, upgrade_rule)| UpgradableComponent {
            component_address,
            upgrade_rule,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use tari_ootle_common_types::{
        SubstateVersion,
        engine_types::{
            component::{ComponentBody, ComponentHeader},
            confidential::ClaimedOutputTombstone,
            substate::{Substate, SubstateValue},
        },
    };
    use tari_ootle_transaction::MigrateFunction;
    use tari_template_lib_types::{
        ClaimedOutputTombstoneAddress,
        EntityId,
        ObjectKey,
        TemplateAddress,
        access_rules::{ComponentAccessRules, RequireRule, RestrictedAccessRule, RuleRequirement},
        crypto::RistrettoPublicKeyBytes,
    };

    use super::*;

    fn address(byte: u8) -> ComponentAddress {
        ComponentAddress::from_array([byte; 32])
    }

    fn owned_component(rule: UpdateRule) -> Component {
        component_with_owner(rule, SubstateOwnerRule::ByPublicKey(RistrettoPublicKeyBytes::default()))
    }

    fn component_with_owner(upgrade_rule: UpdateRule, owner_rule: SubstateOwnerRule) -> Component {
        Component {
            header: ComponentHeader {
                template_address: Default::default(),
                owner_rule,
                access_rules: ComponentAccessRules::new(),
                entity_id: EntityId::default(),
                upgrade_rule,
            },
            body: ComponentBody::empty(),
        }
    }

    fn component_substate(version: u64, rule: UpdateRule) -> Substate {
        Substate::new(
            SubstateVersion::new(version),
            SubstateValue::Component(owned_component(rule)),
        )
    }

    fn cache_with(entries: &[(ComponentAddress, Component)]) -> UpgradeRuleCache {
        let cache = UpgradeRuleCache::new(UPGRADE_RULE_CACHE_MAX_ENTRIES);
        for (address, component) in entries {
            cache.insert(*address, component);
        }
        cache
    }

    fn upgrade(component: ComponentReference, template: u8) -> Instruction {
        Instruction::UpdateComponentTemplate {
            component,
            migrate: None,
            new_template: TemplateAddress::from_array([template; 32]),
        }
    }

    #[test]
    fn every_upgrade_instruction_is_listed_fee_instructions_first() {
        let migrate = MigrateFunction {
            name: "migrate".try_into().unwrap(),
            args: vec![],
        };
        let fee_instructions = [
            Instruction::DropAllProofsInWorkspace,
            upgrade(ComponentReference::Address(address(1)), 10),
        ];
        let instructions = [
            Instruction::UpdateComponentTemplate {
                component: ComponentReference::Workspace(3),
                migrate: Some(migrate),
                new_template: TemplateAddress::from_array([11; 32]),
            },
            Instruction::DropAllProofsInWorkspace,
            upgrade(ComponentReference::Address(address(2)), 12),
        ];

        let upgrades = component_upgrades(&fee_instructions, &instructions);

        let summary = upgrades
            .iter()
            .map(|u| {
                (
                    u.component.clone(),
                    u.new_template,
                    u.migrate.as_ref().map(|m| m.name.to_string()),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(summary, [
            (
                ComponentReference::Address(address(1)),
                TemplateAddress::from_array([10; 32]),
                None
            ),
            (
                ComponentReference::Workspace(3),
                TemplateAddress::from_array([11; 32]),
                Some("migrate".to_string())
            ),
            (
                ComponentReference::Address(address(2)),
                TemplateAddress::from_array([12; 32]),
                None
            ),
        ]);
    }

    #[test]
    fn a_transaction_without_upgrade_instructions_lists_none() {
        assert!(component_upgrades(&[Instruction::DropAllProofsInWorkspace], &[]).is_empty());
    }

    #[test]
    fn filter_upgradable_drops_rules_that_can_never_upgrade_and_keeps_order() {
        let restricted = UpdateRule::AccessRule(AccessRule::Restricted(RestrictedAccessRule::Require(
            RequireRule::Require(RuleRequirement::CallerComponent(address(1))),
        )));

        let upgradable = filter_upgradable([
            (address(1), UpdateRule::Owner),
            (address(2), UpdateRule::Locked),
            (address(3), UpdateRule::AccessRule(AccessRule::AllowAll)),
            (address(4), UpdateRule::AccessRule(AccessRule::DenyAll)),
            (address(5), restricted),
        ]);

        let addresses = upgradable.iter().map(|c| c.component_address).collect::<Vec<_>>();
        assert_eq!(addresses, [address(1), address(3), address(5)]);
    }

    #[test]
    fn an_owner_upgrade_rule_on_an_ownerless_component_is_locked() {
        let ownerless = component_with_owner(UpdateRule::Owner, SubstateOwnerRule::None);
        assert!(matches!(effective_upgrade_rule(&ownerless), UpdateRule::Locked));

        let owned = owned_component(UpdateRule::Owner);
        assert!(matches!(effective_upgrade_rule(&owned), UpdateRule::Owner));

        // An access-rule upgrade does not go through the owner rule.
        let by_rule = component_with_owner(UpdateRule::AccessRule(AccessRule::AllowAll), SubstateOwnerRule::None);
        assert!(matches!(
            effective_upgrade_rule(&by_rule),
            UpdateRule::AccessRule(AccessRule::AllowAll)
        ));
    }

    #[test]
    fn only_components_the_cache_does_not_hold_are_fetched() {
        let cache = cache_with(&[(address(1), owned_component(UpdateRule::Owner))]);

        let (known, uncached) = cached_upgrade_rules(&cache, &[address(2), address(1), address(3), address(2)]);

        assert_eq!(uncached, [address(2), address(3)]);
        assert_eq!(known.keys().copied().collect::<Vec<_>>(), [address(1)]);
        assert!(matches!(known[&address(1)], UpdateRule::Owner));
    }

    #[test]
    fn resolution_uses_the_known_rules_when_the_cache_is_empty() {
        // The cache holds nothing, as after it evicts entries a fetch has just inserted.
        let cache = UpgradeRuleCache::new(1);
        let known = HashMap::from([(address(1), UpdateRule::Owner)]);

        let upgradable = resolve_upgradable_components(&cache, &[address(1)], &known, None);

        assert_eq!(upgradable.iter().map(|c| c.component_address).collect::<Vec<_>>(), [
            address(1)
        ]);
    }

    #[test]
    fn nested_writes_come_from_the_diff_and_read_only_calls_come_from_the_cache() {
        let read_only = address(1);
        let written = address(2);
        let missing = address(3);
        let deny_all = address(4);
        let nested = address(6);
        let created = address(7);
        let ownerless = address(8);

        let cache = cache_with(&[
            (read_only, owned_component(UpdateRule::Owner)),
            // Wrong on purpose: a component the run wrote is taken from the diff.
            (written, owned_component(UpdateRule::Locked)),
            (deny_all, owned_component(UpdateRule::AccessRule(AccessRule::DenyAll))),
            (
                ownerless,
                component_with_owner(UpdateRule::Owner, SubstateOwnerRule::None),
            ),
        ]);
        let called = [read_only, written, read_only, missing, deny_all, ownerless];
        let (known, _) = cached_upgrade_rules(&cache, &called);

        let tombstone =
            SubstateId::ClaimedOutputTombstone(ClaimedOutputTombstoneAddress::new(ObjectKey::from_array([9; 32])));
        let mut diff = SubstateDiff::new();
        diff.down(SubstateId::Component(written), SubstateVersion::ZERO);
        diff.up(SubstateId::Component(written), component_substate(1, UpdateRule::Owner));
        diff.down(tombstone.clone(), SubstateVersion::ZERO);
        diff.up(
            tombstone,
            Substate::new(
                SubstateVersion::new(1),
                SubstateValue::ClaimedOutputTombstone(ClaimedOutputTombstone { value: 1 }),
            ),
        );
        diff.down(SubstateId::Component(nested), SubstateVersion::new(1));
        diff.up(
            SubstateId::Component(nested),
            component_substate(2, UpdateRule::AccessRule(AccessRule::AllowAll)),
        );
        diff.up(SubstateId::Component(created), component_substate(0, UpdateRule::Owner));

        let upgradable = resolve_upgradable_components(&cache, &called, &known, Some(&diff));

        let addresses = upgradable.iter().map(|c| c.component_address).collect::<Vec<_>>();
        assert_eq!(addresses, [read_only, written, nested]);
        assert!(matches!(upgradable[1].upgrade_rule, UpdateRule::Owner));
        assert!(matches!(
            upgradable[2].upgrade_rule,
            UpdateRule::AccessRule(AccessRule::AllowAll)
        ));

        // The cache learns the written components and never the created one.
        assert!(matches!(cache.get(&written), Some(UpdateRule::Owner)));
        assert!(matches!(
            cache.get(&nested),
            Some(UpdateRule::AccessRule(AccessRule::AllowAll))
        ));
        assert!(cache.get(&created).is_none());
    }
}
