//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_abi::{EngineOp, call_engine};
use tari_template_lib::{
    args::{ComponentRef, InvokeResult},
    prelude::*,
    template_macro_deps::minicbor::{self, CborLen, Encode},
};

/// The wire form of `CallerContextInvokeArg` naming the signer-proof action, as an older `tari_template_lib`
/// encoded it.
#[derive(Encode, CborLen)]
struct SignerProofInvokeArg {
    #[n(0)]
    action: SignerProofAction,
    #[n(1)]
    args: Vec<Bytes>,
}

#[derive(Encode, CborLen)]
enum SignerProofAction {
    #[n(2)]
    GetSignerProof,
}

/// The wire form of `ComponentInvokeArg` naming the owner-proof action, as an older `tari_template_lib` encoded it.
#[derive(Encode, CborLen)]
struct OwnerProofInvokeArg {
    #[n(0)]
    component_ref: ComponentRef,
    #[n(1)]
    action: OwnerProofAction,
    #[n(2)]
    args: Vec<Bytes>,
}

#[derive(Encode, CborLen)]
enum OwnerProofAction {
    #[n(5)]
    GetOwnerProof,
}

/// Spends account approvals from inside a component, standing in for a dApp that a spender calls.
#[template]
mod approval_spender_template {
    use super::*;

    pub struct ApprovalSpender {
        vaults: Vec<Vault>,
    }

    impl ApprovalSpender {
        pub fn new() -> Component<Self> {
            Component::new(Self { vaults: Vec::new() })
                .with_access_rules(AccessRules::allow_all())
                .create()
        }

        /// Spends `granter`'s approval with a proof of the transaction signer requested through the signer-proof
        /// engine call.
        pub fn spend_with_signer_proof(
            &self,
            granter: ComponentAddress,
            resource: ResourceAddress,
            amount: Amount,
            dest: ComponentAddress,
        ) {
            let result: InvokeResult = call_engine(EngineOp::CallerContextInvoke, &SignerProofInvokeArg {
                action: SignerProofAction::GetSignerProof,
                args: vec![],
            });
            let proof: Proof = result.decode().unwrap();
            Self::spend_with_proof(granter, proof, resource, amount, dest);
        }

        /// Spends `granter`'s approval with a proof of the owner of `signer_account` requested through the
        /// owner-proof engine call.
        pub fn spend_with_owner_proof(
            &self,
            signer_account: ComponentAddress,
            granter: ComponentAddress,
            resource: ResourceAddress,
            amount: Amount,
            dest: ComponentAddress,
        ) {
            let result: InvokeResult = call_engine(EngineOp::ComponentInvoke, &OwnerProofInvokeArg {
                component_ref: ComponentRef::Ref(signer_account),
                action: OwnerProofAction::GetOwnerProof,
                args: vec![],
            });
            let proof: Option<Proof> = result.decode().unwrap();
            Self::spend_with_proof(granter, proof.unwrap(), resource, amount, dest);
        }

        fn spend_with_proof(
            granter: ComponentAddress,
            proof: Proof,
            resource: ResourceAddress,
            amount: Amount,
            dest: ComponentAddress,
        ) {
            let bucket: Bucket =
                ComponentManager::get(granter).call("withdraw_approved", args![proof.clone(), resource, amount]);
            ComponentManager::get(dest).invoke("deposit", args![bucket]);
            proof.drop();
        }

        /// Spends `granter`'s approval naming `spender_badge`, which this component's call to the granter does not
        /// carry unless it is this component's own caller badge.
        pub fn spend_naming_badge(
            &self,
            granter: ComponentAddress,
            spender_badge: NonFungibleAddress,
            resource: ResourceAddress,
            amount: Amount,
            dest: ComponentAddress,
        ) {
            let bucket: Bucket = ComponentManager::get(granter).call("withdraw_approved", args![
                spender_badge,
                resource,
                amount
            ]);
            ComponentManager::get(dest).invoke("deposit", args![bucket]);
        }

        /// Mints a single non-fungible badge into `account` and returns its address.
        pub fn mint_badge_into(account: ComponentAddress) -> NonFungibleAddress {
            let id = NonFungibleId::from_u32(1);
            let bucket = ResourceBuilder::non_fungible().initial_supply([id.clone()]);
            let badge = NonFungibleAddress::new(bucket.resource_address(), id);
            ComponentManager::get(account).invoke("deposit", args![bucket]);
            badge
        }

        /// Pulls `amount` from `granter` under an approval naming this component, and keeps it.
        pub fn pull(&mut self, granter: ComponentAddress, resource: ResourceAddress, amount: Amount) {
            let badge = NonFungibleAddress::caller_component_badge(CallerContext::current_component_address());
            let bucket: Bucket =
                ComponentManager::get(granter).call("withdraw_approved", args![badge, resource, amount]);
            self.vaults.push(Vault::from_bucket(bucket));
        }
    }
}
