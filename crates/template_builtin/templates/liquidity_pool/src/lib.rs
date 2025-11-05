//   Copyright 2023. The Tari Project
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

// TODO: note this template is WIP and not currently used anywhere

#![no_std]
extern crate alloc;

use tari_template_lib::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pool {
    A,
    B,
}

#[template]
mod template {
    use alloc::string::ToString;

    use tari_template_lib::resource::TOKEN_SYMBOL;

    use super::*;

    pub struct TwoResourceLiquidityPool {
        vault_a: Vault,
        vault_b: Vault,
        lp_resource: ResourceManager,
    }

    impl TwoResourceLiquidityPool {
        // Creates a new two-resource liquidity pool component for the resources A and B
        pub fn instantiate(
            owner_rule: OwnerRule,
            pool_token_rules: AccessRule,
            a_addr: ResourceAddress,
            b_addr: ResourceAddress,
            mut metadata: Metadata,
            address_allocation: Option<ComponentAddressAllocation>,
        ) -> Component<Self> {
            // check that the resource pair is correct
            assert_ne!(a_addr, b_addr, "The resources of the pair must be different");
            Self::check_resource_is_fungible(a_addr);
            Self::check_resource_is_fungible(b_addr);

            // Set token symbol to "LP" if not provided
            metadata.get_or_insert(TOKEN_SYMBOL, "LP");
            // create the lp resource
            let lp_resource = ResourceBuilder::public_fungible()
                .with_divisibility(0)
                .with_access_rules(
                    ResourceAccessRules::new()
                        .mintable(pool_token_rules.clone())
                        .burnable(pool_token_rules.clone()),
                )
                .with_owner_rule(owner_rule.clone())
                .with_metadata(metadata)
                .build();

            let vault_a = Vault::new_empty(a_addr);
            let vault_b = Vault::new_empty(b_addr);

            Component::new(Self {
                vault_a,
                vault_b,
                lp_resource: ResourceManager::get(lp_resource),
            })
            .with_owner_rule(owner_rule)
            .with_address_allocation_opt(address_allocation)
            .with_access_rules(
                ComponentAccessRules::new()
                    .add_method_rule("add_liquidity", pool_token_rules.clone())
                    .add_method_rule("remove_liquidity", pool_token_rules.clone())
                    .add_method_rule("contribute", pool_token_rules)
                    .default(AccessRule::AllowAll),
            )
            .create()
        }

        pub fn contribute(&mut self, mut bucket_a: Bucket, mut bucket_b: Bucket) -> Bucket {
            // Potentially saves binary space
            const OVERFLOW_MSG: &str = "Overflow when calculating LP token mint amount";

            // check that the buckets are correct
            let resource_a = bucket_a.resource_address();
            let resource_b = bucket_b.resource_address();
            assert_ne!(resource_a, resource_b, "The resources must be different");

            // contribution amounts
            let a_amount = bucket_a.amount();
            let b_amount = bucket_b.amount();

            if a_amount.is_zero() || b_amount.is_zero() {
                panic!("Cannot contribute zero amount of liquidity");
            }

            let pool_a = self.get_pool_from_resource(resource_a);
            let pool_b = self.get_pool_from_resource(resource_b);

            // Get the reserves for each pool
            let reserve_a = self.get_pool_vault(pool_a).balance();
            let reserve_b = self.get_pool_vault(pool_b).balance();

            let lp_supply = self.lp_total_supply();

            let (lp_mint_amount, a_contribution, b_contribution) = match (
                lp_supply.is_positive(),
                reserve_a.is_positive(),
                reserve_b.is_positive(),
            ) {
                // Reserve pools are empty, mint initial lp tokens
                (false, false, false) => {
                    // initial liquidity provision, mint lp tokens equal to the geometric mean  `sqrt(c_1 * c_2)` of the
                    // contributions

                    (
                        // TODO: lost of precision
                        a_amount
                            .checked_mul(b_amount)
                            .and_then(|c1_c2| c1_c2.checked_sqrt())
                            .expect(OVERFLOW_MSG),
                        a_amount,
                        b_amount,
                    )
                },
                (false, _, _) => {
                    // No LP tokens currently exist but reserves are non-zero
                    // Calculate the geometric mean of the ratios of the contributions to the reserves
                    // i.e. sqrt(c_1 + r_1) * sqrt(c_2 + r_2)

                    // TODO: lost of precision
                    let mint = a_amount
                        .checked_add(reserve_a)
                        .and_then(|c1_r1| c1_r1.checked_sqrt())
                        .and_then(|sqrt_c1_r1| {
                            b_amount
                                .checked_add(reserve_b)
                                .and_then(|c2_r2| c2_r2.checked_sqrt())
                                .and_then(|sqrt_c2_r2| sqrt_c1_r1.checked_mul(sqrt_c2_r2))
                        })
                        .expect(OVERFLOW_MSG);

                    (mint, a_amount, b_amount)
                },
                (true, true, true) => {
                    // Normal case: existing LP supply and non-zero reserves
                    // calculate the amount each contribution can be contributed to keep the ratio of the reserves the
                    // same

                    let required_contribution_a = a_amount
                        .checked_div(reserve_a)
                        .and_then(|r| r.checked_mul(reserve_b))
                        .map(|b_required| (a_amount, b_required))
                        .expect(OVERFLOW_MSG);

                    let required_contribution_b = b_amount
                        .checked_div(reserve_b)
                        .and_then(|r| r.checked_mul(reserve_a))
                        .map(|a_required| (a_required, b_amount))
                        .expect(OVERFLOW_MSG);

                    [required_contribution_a, required_contribution_b]
                        .into_iter()
                        // filter only the contributions that are less than or equal to what the user provided
                        .filter(|(c_a, c_b)| *c_a <= a_amount && *c_b <= b_amount)
                        .map(|(c_a, c_b)| {
                            let mint = c_a
                                .checked_div(reserve_a)
                                .and_then(|r| r.checked_mul(lp_supply))
                                .expect(OVERFLOW_MSG);
                            (mint, c_a, c_b)
                        })
                        .max_by(|(mint_a, _, _), (mint_b, _, _)| mint_a.cmp(mint_b))
                        .expect("Insufficient contribution amounts for existing pool reserves")
                },
                (true, _, _) => {
                    // One or both reserves are zero with existing LP supply. LP Supply should always represent
                    // reserves, so this is an inconsistent state.
                    panic!("Inconsistent pool state: non-zero LP supply with zero reserve");
                },
            };

            // mint and return the new lp tokens
            let mint_lp_tokens = self.lp_resource.mint_fungible(lp_mint_amount);

            let contributed_a = bucket_a.take(a_contribution);
            let contributed_b = bucket_b.take(b_contribution);

            // deposit the liquidity to the pool
            self.vault_a.deposit(contributed_a);
            self.vault_b.deposit(contributed_b);

            emit_event("contribute", metadata![
                "resource_a" => resource_a.to_string(),
                "resource_b" => resource_b.to_string(),
                "minted_lp_tokens" => lp_mint_amount.to_string(),
                "contributed_a" => a_contribution.to_string(),
                "contributed_b" => b_contribution.to_string(),
            ]);

            mint_lp_tokens
        }

        pub fn redeem(&mut self, lp_bucket: Bucket) -> (Bucket, Bucket) {
            // check that the bucket is correct
            let lp_resource = self.lp_resource.resource_address();
            let bucket_resource = lp_bucket.resource_address();
            if bucket_resource != lp_resource {
                panic!(
                    "The provided bucket {} must be pool LP resource {}",
                    bucket_resource, lp_resource
                );
            }

            let redeem_amount = lp_bucket.amount();
            let lp_total_supply = self.lp_total_supply();

            let (a_amount, b_amount) = self.calculate_redemption_amounts(lp_total_supply, redeem_amount);

            // withdraw the redeemed amounts from the pool
            let a_bucket = self.vault_a.withdraw(a_amount);
            let b_bucket = self.vault_b.withdraw(b_amount);

            // burn the redeemed lp tokens
            lp_bucket.burn();

            emit_event("redeem", [
                ("lp_redeemed", redeem_amount.to_string()),
                ("resource_a", a_bucket.resource_address().to_string()),
                ("amount_a", a_amount.to_string()),
                ("resource_b", b_bucket.resource_address().to_string()),
                ("amount_b", b_amount.to_string()),
            ]);

            (a_bucket, b_bucket)
        }

        pub fn add_liquidity(&mut self, bucket: Bucket) {
            // check that the buckets are correct
            let resource = bucket.resource_address();
            emit_event("add_liquidity", [
                ("resource_address", resource.to_string()),
                ("amount", bucket.amount().to_string()),
            ]);

            // add the liquidity to the pool
            let pool = self.get_pool_from_resource(resource);
            self.get_pool_vault(pool).deposit(bucket);
        }

        pub fn remove_liquidity(&mut self, resource_address: ResourceAddress, amount: Amount) -> Bucket {
            let pool = self.get_pool_from_resource(resource_address);
            emit_event("remove_liquidity", [
                ("resource_address", resource_address.to_string()),
                ("amount", amount.to_string()),
            ]);

            self.get_pool_vault(pool).withdraw(amount)
        }

        pub fn get_redemption_value(&self, lp_redeem_amount: Amount) -> (Amount, Amount) {
            let lp_total_supply = self.lp_total_supply();

            if !lp_redeem_amount.is_positive() || lp_redeem_amount > lp_total_supply {
                panic!(
                    "Invalid redemption amount {} (total supply {})",
                    lp_redeem_amount, lp_total_supply
                );
            }

            self.calculate_redemption_amounts(lp_total_supply, lp_redeem_amount)
        }

        fn calculate_redemption_amounts(&self, lp_total_supply: Amount, lp_redeem_amount: Amount) -> (Amount, Amount) {
            // get the pool reserve
            let a_reserve = self.vault_a.balance();
            let b_reserve = self.vault_b.balance();

            // calculate the amounts owed to the user based on provided LP tokens
            let lp_ratio = lp_redeem_amount.checked_div(lp_total_supply).expect("Div zero");
            let a_amount_owed = lp_ratio
                .checked_mul(a_reserve)
                .expect("Amount overflow when calculating redemption value for resource A");

            let b_amount_owed = lp_ratio
                .checked_mul(b_reserve)
                .expect("Amount overflow when calculating redemption value for resource B");

            (a_amount_owed, b_amount_owed)
        }

        pub fn get_pool_balances(&self) -> (Amount, Amount) {
            (self.vault_a.balance(), self.vault_b.balance())
        }

        fn get_pool_from_resource(&self, resource_address: ResourceAddress) -> Pool {
            if self.vault_a.resource_address() == resource_address {
                Pool::A
            } else if self.vault_b.resource_address() == resource_address {
                Pool::B
            } else {
                panic!("Resource {} is not in the pool", resource_address);
            }
        }

        fn get_pool_vault(&self, pool: Pool) -> &Vault {
            match pool {
                Pool::A => &self.vault_a,
                Pool::B => &self.vault_b,
            }
        }

        fn lp_total_supply(&self) -> Amount {
            self.lp_resource.total_supply()
        }

        fn check_resource_is_fungible(resource: ResourceAddress) {
            let resource_type = ResourceManager::get(resource).resource_type();
            assert!(
                matches!(
                    resource_type,
                    ResourceType::Fungible | ResourceType::Confidential | ResourceType::Stealth
                ),
                "Resource {} is not fungible (Fungible, Stealth, Confidential)",
                resource
            );
        }
    }
}
