//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! The `SetComponentAccessRules` and `SetComponentOwnerRule` instructions in a transaction, for whoever signs or
//! approves it.
//!
//! A signer who satisfies a component's rules updater decides, through these instructions, who may call each of its
//! methods or who owns it. An owner rule change hands the component, and every vault it holds, to the new owner.

use axum_extra::headers::authorization::Bearer;
use tari_ootle_transaction::Instruction;
use tari_ootle_walletd_client::{permissions::Permission, types::ComponentRulesChange};

use super::{auth::jwt::AuthError, component_upgrades::display_component_reference, context::HandlerContext};

/// Every `SetComponentAccessRules` and `SetComponentOwnerRule` instruction in the transaction, fee instructions first.
pub fn component_rules_changes(
    fee_instructions: &[Instruction],
    instructions: &[Instruction],
) -> Vec<ComponentRulesChange> {
    fee_instructions
        .iter()
        .chain(instructions)
        .filter_map(|instruction| match instruction {
            Instruction::SetComponentAccessRules {
                component,
                access_rules,
            } => Some(ComponentRulesChange::AccessRules {
                component: component.clone(),
                access_rules: access_rules.clone(),
            }),
            Instruction::SetComponentOwnerRule { component, owner_rule } => Some(ComponentRulesChange::OwnerRule {
                component: component.clone(),
                owner_rule: owner_rule.clone(),
            }),
            _ => None,
        })
        .collect()
}

/// Refuses a submission that changes a component's rules unless the caller holds `admin`.
///
/// `transactions:create` lets a tool have the wallet sign arbitrary transactions with no person reviewing them. A
/// rules change signed by a key that satisfies the component's rules updater can hand the component, and every vault
/// it holds, to someone else, so it needs the credential a person keeps for themselves. Transaction and signing
/// requests are gated by a person's approval instead, made after seeing [`component_rules_changes`].
pub fn authorize_component_rules_changes(
    context: &HandlerContext,
    token: Option<&Bearer>,
    changes: &[ComponentRulesChange],
) -> Result<(), AuthError> {
    let Some(change) = changes.first() else {
        return Ok(());
    };
    match context.authorize(token, &[Permission::Admin]) {
        Ok(_) => Ok(()),
        Err(AuthError::InsufficientPermissions { .. }) => Err(AuthError::ComponentRulesChangeRequiresAdmin {
            component: display_component_reference(change.component()),
            rules: match change {
                ComponentRulesChange::AccessRules { .. } => "access rules",
                ComponentRulesChange::OwnerRule { .. } => "owner rule",
            },
        }),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use tari_ootle_transaction::ComponentReference;
    use tari_template_lib_types::{ComponentAddress, OwnerRule, access_rules::ComponentAccessRules};

    use super::*;

    fn address(byte: u8) -> ComponentReference {
        ComponentReference::Address(ComponentAddress::from_array([byte; 32]))
    }

    #[test]
    fn every_rules_change_instruction_is_listed_fee_instructions_first() {
        let fee_instructions = [
            Instruction::DropAllProofsInWorkspace,
            Instruction::SetComponentOwnerRule {
                component: address(1),
                owner_rule: OwnerRule::None,
            },
        ];
        let instructions = [
            Instruction::SetComponentAccessRules {
                component: ComponentReference::Workspace(3),
                access_rules: ComponentAccessRules::new(),
            },
            Instruction::DropAllProofsInWorkspace,
            Instruction::SetComponentOwnerRule {
                component: address(2),
                owner_rule: OwnerRule::OwnedBySigner,
            },
        ];

        let changes = component_rules_changes(&fee_instructions, &instructions);

        let summary = changes
            .iter()
            .map(|change| match change {
                ComponentRulesChange::AccessRules { component, .. } => (component.clone(), "access"),
                ComponentRulesChange::OwnerRule { component, .. } => (component.clone(), "owner"),
            })
            .collect::<Vec<_>>();
        assert_eq!(summary, [
            (address(1), "owner"),
            (ComponentReference::Workspace(3), "access"),
            (address(2), "owner"),
        ]);
    }

    #[test]
    fn a_transaction_without_rules_change_instructions_lists_none() {
        assert!(component_rules_changes(&[Instruction::DropAllProofsInWorkspace], &[]).is_empty());
    }
}
