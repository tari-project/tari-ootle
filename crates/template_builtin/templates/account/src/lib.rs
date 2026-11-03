//   Copyright 2022. The Tari Project
//
//   Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//   following conditions are met:
//
//   1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//   disclaimer.
//
//   2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//   following disclaimer in the documentation and/or other materials provided with the distribution.
//
//   3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//   products derived from this software without specific prior written permission.
//
//   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//   INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//   DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//   SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//   SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//   WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//   USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use tari_template_lib::prelude::*;

type VaultMap = PrehashedMap<ResourceAddress, Vault>;
type ApprovalMap = PrehashedMap<(ResourceAddress, NonFungibleAddress), Amount>;

/// The maximum number of vaults an account may hold. Anyone may deposit into an account and vaults can never be
/// removed, while every call on the account decodes its whole vault map, and every `&mut self` call re-encodes it,
/// inside the WASM memory limit. The map must stay bounded well below what that limit can hold.
const MAX_VAULTS: usize = 1024;

#[template]
mod account_template {
    use super::*;

    pub struct Account {
        #[cbor(with = "indexmap_codec")]
        vaults: VaultMap,
        #[cbor(with = "indexmap_codec")]
        /// Approvals for others to withdraw from this account. The key is a tuple of (approved resource,
        /// required spender_badge), and the value is the approved amount. See [`Self::withdraw_approved`] for how a
        /// spender shows that it holds the badge.
        approvals: ApprovalMap,
    }

    impl Account {
        pub fn create(
            public_key_token: NonFungibleAddress,
            owner_rule: Option<OwnerRule>,
            access_rules: Option<AccessRules>,
            bucket: Option<Bucket>,
        ) -> Component<Account> {
            // extract the public key from the token
            // we only allow tokens that correspond to public keys
            let public_key = public_key_token
                .to_public_key()
                .unwrap_or_else(|| panic!("public_key_token is not a valid public key: {}", public_key_token));

            // The owner of this account is either provided explicitly or defaults to the provided public key.
            let owner_rule = owner_rule.unwrap_or(OwnerRule::ByPublicKey(public_key));

            let access_rules = access_rules.unwrap_or(
                // By default, allow deposits from anyone
                AccessRules::new()
                    .add_method_rule("balance", rule!(allow_all))
                    .add_method_rule("get_balances", rule!(allow_all))
                    .add_method_rule("deposit", rule!(allow_all))
                    .add_method_rule("withdraw_approved", rule!(allow_all))
                    // By default, only the owner of the token will be able to withdraw funds from the account
                    .default(rule!(deny_all)),
            );

            // add the funds from the (optional) bucket
            let mut vaults = VaultMap::default();
            if let Some(b) = bucket {
                vaults.insert(b.resource_address(), Vault::from_bucket(b));
            }

            Component::new(Self {
                vaults,
                approvals: ApprovalMap::default(),
            })
            .with_access_rules(access_rules)
            .with_public_key_address(public_key)
            .with_owner_rule(owner_rule)
            .create()
        }

        pub fn balance(&self, resource: ResourceAddress) -> Amount {
            self.vaults
                .get(&resource)
                .map(|v| v.balance())
                .unwrap_or_else(Amount::zero)
        }

        /// Only applies to confidential resources. Returns the number of commitments in the vault.
        pub fn confidential_commitment_count(&self, resource: ResourceAddress) -> u32 {
            self.get_vault(resource).commitment_count()
        }

        pub fn withdraw(&self, resource: ResourceAddress, amount: Amount) -> Bucket {
            // An event is emitted by the vault.withdraw method
            let v = self.get_vault(resource);
            v.withdraw(amount)
        }

        pub fn withdraw_all(&self, resource: ResourceAddress) -> Bucket {
            let v = self.get_vault(resource);
            v.withdraw(v.balance())
        }

        pub fn withdraw_non_fungible(&self, resource: ResourceAddress, nf_id: NonFungibleId) -> Bucket {
            // An event is emitted by the vault.withdraw_non_fungibles method
            let v = self.get_vault(resource);
            v.withdraw_non_fungibles([nf_id])
        }

        pub fn withdraw_many_non_fungibles(&self, resource: ResourceAddress, nf_ids: Vec<NonFungibleId>) -> Bucket {
            // An event is emitted by the vault.withdraw_non_fungibles method
            let v = self.get_vault(resource);
            v.withdraw_non_fungibles(nf_ids)
        }

        pub fn withdraw_confidential(
            &self,
            resource: ResourceAddress,
            withdraw_proof: ConfidentialWithdrawProof,
        ) -> Bucket {
            // An event is emitted by the vault.withdraw_confidential method
            let v = self.get_vault(resource);
            v.withdraw_confidential(withdraw_proof)
        }

        pub fn deposit(&mut self, bucket: Bucket) {
            // Ignore empty buckets
            if bucket.is_empty() {
                bucket.drop_empty();
                return;
            }
            // An event is emitted by the vault.deposit method
            let resource_address = bucket.resource_address();
            if let Some(vault) = self.vaults.get(&resource_address) {
                vault.deposit(bucket);
                return;
            }
            assert!(
                self.vaults.len() < MAX_VAULTS,
                "Account holds the maximum of {MAX_VAULTS} vaults and cannot accept new resource {resource_address}. \
                 Deposit into another account."
            );
            self.vaults.insert(resource_address, Vault::from_bucket(bucket));
        }

        fn get_vault(&self, resource: ResourceAddress) -> &Vault {
            self.vaults
                .get(&resource)
                .unwrap_or_else(|| panic!("No vault for resource {}", resource))
        }

        pub fn get_balances(&self) -> Vec<(ResourceAddress, Amount)> {
            self.vaults.iter().map(|(k, v)| (*k, v.balance())).collect()
        }

        /// Withdraws funds using the ConfidentialWithdrawProof, and immediately deposits the withdrawal back into the
        /// vault. It will panic if the proof is invalid or the resource type contained in the vault is not
        /// confidential. This is useful for converting confidential tokens into revealed tokens and vice versa.
        pub fn join_confidential(&self, resource: ResourceAddress, proof: ConfidentialWithdrawProof) {
            // An event is emitted by the vault.withdraw_confidential and vault.deposit methods
            let vault = self.get_vault(resource);
            let bucket = vault.withdraw_confidential(proof);
            vault.deposit(bucket);
        }

        // Fee methods. These are used to pay fees and satisfy a "duck-typed" interface.

        /// Pay fees from previously revealed stealth resource.
        pub fn pay_fee(&self, amount: Amount) {
            self.get_vault(STEALTH_TARI_RESOURCE_ADDRESS).pay_fee(amount);
        }

        /// Reveal stealth tokens and return the revealed bucket to pay fees.
        pub fn pay_fee_stealth(&self, transfer: StealthTransferStatement) {
            self.get_vault(STEALTH_TARI_RESOURCE_ADDRESS)
                .pay_fee_stealth(transfer);
        }

        pub fn create_proof_for_resource(&self, resource: ResourceAddress) -> Proof {
            let v = self.get_vault(resource);
            v.create_proof()
        }

        pub fn create_proof_by_non_fungible(&self, nft: NonFungibleAddress) -> Proof {
            self.create_proof_by_non_fungible_ids(*nft.resource_address(), vec![nft.id().clone()])
        }

        pub fn create_proof_by_non_fungible_ids(
            &self,
            resource: ResourceAddress,
            ids: Vec<NonFungibleId>,
        ) -> Proof {
            let v = self.get_vault(resource);
            v.create_proof_by_non_fungible_ids(ids.into_iter().collect())
        }

        pub fn create_proof_by_amount(&self, resource: ResourceAddress, amount: Amount) -> Proof {
            let v = self.get_vault(resource);
            v.create_proof_by_amount(amount)
        }

        // Approval methods
        /// Called by account owner — sets or updates approval. A zero `amount` revokes it.
        pub fn approve(&mut self, spender_badge: NonFungibleAddress, resource: ResourceAddress, amount: Amount) {
            if amount.is_zero() {
                let key = (resource, spender_badge);
                if self.approvals.swap_remove(&key).is_some() {
                    emit_event("revoke_approval", metadata![
                        "resource" => key.0,
                        "spender_badge" => key.1,
                    ]);
                }
                // Else no-op
            } else {
                emit_event(
                    "approve",
                    metadata!(
                        "spender_badge" => spender_badge,
                        "resource" => resource,
                        "amount" => amount,
                    ),
                );

                self.approvals.insert((resource, spender_badge), amount);
            }
        }

        pub fn revoke_all_approvals(&mut self) {
            self.approvals.clear();
            emit_event("revoke_all_approvals", metadata!());
        }

        /// Withdraws `amount` under the approval for `spender_badge`, which this call must carry: a public-key badge
        /// when that key's transaction calls this method directly, or, when template code makes the call, the
        /// caller-component badge of the calling component or the caller-template badge of its template. An
        /// approval keyed on a template badge is therefore spendable by every component and function of that
        /// template. A badge held in a vault is carried by authorizing a proof of it around this call.
        pub fn withdraw_approved(
            &mut self,
            spender_badge: NonFungibleAddress,
            resource: ResourceAddress,
            amount: Amount,
        ) -> Bucket {
            CallerContext::assert_badge_in_scope(&spender_badge);

            let approval = self
                .approvals
                .get_mut(&(resource, spender_badge.clone()))
                .unwrap_or_else(|| panic!("No approval for badge {} on resource {}", spender_badge, resource));

            *approval = approval
                .checked_sub(amount)
                .unwrap_or_else(|| panic!("Amount exceeds approval (max: {}, attempted: {})", approval, amount));
            let is_exhausted = approval.is_zero();

            emit_event("withdraw_approved", metadata![
                "spender_badge" => spender_badge,
                "resource" => resource,
                "amount" => amount,
            ]);

            // Clean up zero approvals
            if is_exhausted {
                self.approvals.swap_remove(&(resource, spender_badge));
            }

            self.get_vault(resource).withdraw(amount)
        }
    }
}
