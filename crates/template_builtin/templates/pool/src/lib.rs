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

#[derive(Clone, Copy)]
enum Pool {
    A,
    B,
}

// #[template]
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
        pub fn new(
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
                    .add_method_rule("remove_liquidity", pool_token_rules)
                    .default(AccessRule::DenyAll),
            )
            .create()
        }

        /// swap A tokens for B tokens or vice-versa
        pub fn swap(&mut self, input_bucket: Bucket) -> Bucket {
            // check that the parameters are correct
            let input_resource = input_bucket.resource_address();
            self.assert_pool_resource(input_resource);

            let output_resource = if input_resource == self.get_a_resource() {
                self.get_b_resource()
            } else {
                self.get_a_resource()
            };

            // get the data needed to calculate the pool rebalancing
            let input_vault = self.get_pool_vault(input_resource);
            let input_pool_balance = input_vault.balance();
            let output_vault = self.get_pool_vault(output_resource);
            let output_pool_balance = output_vault.balance();

            // check that the pools are not empty, to prevent division by 0 errors later
            assert!(
                !input_pool_balance.is_zero(),
                "The pool for resource '{}' is empty",
                input_resource
            );
            assert!(
                !output_pool_balance.is_zero(),
                "The pool for resource '{}' is empty",
                output_resource
            );

            // so the user will get a lesser amount of tokens than the theoretical (for the gain of the LP holders)
            let input_bucket_balance = input_bucket.amount();
            let effective_input_balance = input_bucket_balance; // - (input_bucket_balance * self.fee.into()) / 1000.into();

            // recalculate the new vault balances for the swap
            // constant product AMM formula is "k = a * b"
            // so the new output vault balance should be "b = k / a"
            let k = input_pool_balance * output_pool_balance;
            let new_input_pool_balance = input_pool_balance + effective_input_balance;
            let new_output_pool_balance = k / new_input_pool_balance;

            // calculate the amount of output tokens to return to the user
            let output_bucket_amount = output_pool_balance - new_output_pool_balance;

            // perform the swap
            input_vault.deposit(input_bucket);
            output_vault.withdraw(output_bucket_amount)
        }

        pub fn contribute(&mut self, bucket_a: Bucket, bucket_b: Bucket) -> Bucket {
            // check that the buckets are correct
            let resource_a = bucket_a.resource_address();
            let resource_b = bucket_b.resource_address();
            self.assert_pool_resource(resource_a);
            self.assert_pool_resource(resource_b);
            assert_ne!(resource_a, resource_b, "The resources must be different");

            // extract the bucket amounts for later
            let a_amount = bucket_a.amount();
            let b_amount = bucket_b.amount();

            // add the liquidity to the pool
            self.vault_a.deposit(bucket_a);
            self.vault_b.deposit(bucket_b);

            // get the bucket/pool ratios
            let a_ratio = self.get_a_ratio(a_amount);
            let b_ratio = self.get_b_ratio(b_amount);

            // the amount of new lp tokens are proportional to the bucket-pool ratios
            let new_lp_amount = a_ratio * a_amount + b_ratio * b_amount;

            // mint and return the new lp tokens
            self.lp_resource.mint_fungible(new_lp_amount)
        }

        pub fn redeem(&mut self, lp_bucket: Bucket) -> (Bucket, Bucket) {
            let lp_amount = lp_bucket.amount();

            // get the pool information
            let a_balance = self.vault_a.balance();
            let b_balance = self.vault_b.balance();

            // calculate the amount of tokens to take from each pool
            let decimals = Amount::from(1_000_000u64);
            let lp_ratio = (lp_amount * decimals) / (self.lp_total_supply() * decimals);

            // TODO: div_ceil is probably not a great rounding function
            let a_amount = (lp_ratio.div_ceil(decimals)) * a_balance;
            let b_amount = (lp_ratio.div_ceil(decimals)) * b_balance;

            // burn the LP tokens
            lp_bucket.burn();
            // return the pool tokens
            let a_bucket = self.vault_a.withdraw(a_amount);
            let b_bucket = self.vault_b.withdraw(b_amount);
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

        pub fn get_redemption_value(&self, lp_amount: Amount) -> (Amount, Amount) {
            // get the pool information
            let a_balance = self.vault_a.balance();
            let a_decimals = self.vault_a.to_resource_manager().divisibility();
            let b_balance = self.vault_b.balance();
            let b_decimals = self.vault_b.to_resource_manager().divisibility();

            // calculate the amount of tokens to take from each pool
            let decimals = Amount::TEN.pow(u8::max(a_decimals, b_decimals) as u32);
            let lp_ratio = (lp_amount * decimals) / (self.lp_total_supply() * decimals);
            let a_amount = (lp_ratio.div_ceil(decimals)) * a_balance;
            let b_amount = (lp_ratio.div_ceil(decimals)) * b_balance;
            (a_amount, b_amount)
        }

        pub fn get_a_resource(&self) -> ResourceAddress {
            self.vault_a.resource_address()
        }

        pub fn get_b_resource(&self) -> ResourceAddress {
            self.vault_b.resource_address()
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

        pub fn get_a_ratio(&self, amount: Amount) -> Amount {
            let balance = self.vault_a.balance();

            if balance == 0 {
                Amount::ONE
            } else {
                amount.checked_div(balance).expect("Division by zero in get_a_ratio")
            }
        }

        pub fn get_b_ratio(&self, amount: Amount) -> Amount {
            let balance = self.vault_b.balance();

            if balance.is_zero() {
                Amount::ONE
            } else {
                amount.checked_div(balance).expect("Division by zero in get_b_ratio")
            }
        }

        pub fn get_k(&self) -> Amount {
            self.vault_a.balance() * self.vault_b.balance()
        }

        pub fn get_a_price_in_b(&self) -> Amount {
            if self.vault_b.balance().is_zero() {
                Amount::ZERO
            } else {
                self.get_k() / self.vault_b.balance()
            }
        }

        pub fn lp_resource(&self) -> ResourceAddress {
            self.lp_resource.resource_address()
        }

        pub fn lp_total_supply(&self) -> Amount {
            self.lp_resource.total_supply()
        }

        fn assert_pool_resource(&self, resource: ResourceAddress) {
            assert!(
                self.vault_a.resource_address() == resource || self.vault_b.resource_address() == resource,
                "The resource {} is not in the pool",
                resource
            );
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
