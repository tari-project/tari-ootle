//  Copyright 2023. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use tari_template_lib::prelude::*;

#[template]
mod template {
    use tari_template_lib::invoke_args;

    use super::*;

    pub struct CrossTemplate {
        // we assume the inner component is a "State" template component
        state_component: ComponentManager,

        // we optionally store other composability components just to test recursion limits
        nested_composability: Option<ComponentAddress>,
    }

    impl CrossTemplate {
        // function-to-function call
        // both "cross template" and "state" components are created
        pub fn new(state_template_address: TemplateAddress) -> Component<Self> {
            let state_component_address = TemplateManager::get(state_template_address).call("new", args![]);
            Component::new(Self {
                state_component: ComponentManager::get(state_component_address),
                nested_composability: None,
            })
            .with_access_rules(AccessRules::allow_all())
            .create()
        }

        // function-to-component call
        // the argument is a "CrossTemplate" component, we get the "State" component address from it
        pub fn new_from_component(
            address_alloc: ComponentAddressAllocation,
            other_composability_component_address: ComponentAddress,
        ) -> Component<Self> {
            let state_component_address = ComponentManager::get(other_composability_component_address)
                .call("get_state_component_address", args![]);
            Component::new(Self {
                state_component: ComponentManager::get(state_component_address),
                nested_composability: None,
            })
            .with_address_allocation(address_alloc)
            .with_access_rules(AccessRules::allow_all())
            .create()
        }

        pub fn get_state_component_address(&self) -> ComponentAddress {
            self.state_component.component_address()
        }

        pub fn set_nested_composability(&mut self, address: ComponentAddress) {
            self.nested_composability = Some(address);
        }

        // component-to-component call
        pub fn increase_inner_state_component(&self) {
            // read operation, to get the current value of the inner "State" component
            let value: u32 = self.state_component.call("get", args![]);

            // write operation, to update the value of the inner "State" component
            self.state_component.call("set".to_string(), args![value + 1])
        }

        // function-to-component call
        pub fn replace_state_component(&mut self, state_template_address: TemplateAddress) {
            let new_address = TemplateManager::get(state_template_address).call("new".to_string(), args![]);
            self.state_component = ComponentManager::get(new_address);
        }

        pub fn call_method_that_does_not_exist(&self) {
            self.state_component
                .call("this_method_does_not_exist".to_string(), args![])
        }

        // malicious method, that tries to withdraw from caller's account
        // the engine should fail any call to this method
        pub fn malicious_withdraw(
            &self,
            victim_account_address: ComponentAddress,
            resource_address: ResourceAddress,
            amount: Amount,
        ) {
            let account = ComponentManager::get(victim_account_address);

            // we try to withdraw the funds, this operation SHOULD fail due to insufficient permissions
            let bucket: Bucket = account.call("withdraw", args![resource_address, amount]);

            // we are going to return back the funds so the call does not fail for "dangling buckets" reason
            // but if the previous operation does execute, this means we could have sent the funds to any other account
            account.call("deposit", args![bucket])
        }

        // recursive function used to test recursion depth limits
        pub fn get_nested_value(&self) -> u32 {
            match self.nested_composability {
                Some(addr) => {
                    // recursive call to the nested composability component
                    ComponentManager::get(addr).call("get_nested_value", args![])
                },
                None => {
                    // base case that will end a recursive call chain
                    self.state_component.call("get", args![])
                },
            }
        }

        pub fn recursion(&self, depth: usize) {
            if depth == 0 {
                return;
            }
            ComponentManager::current().invoke("recursion", args![depth - 1])
        }

        pub fn call_component_with_args(
            component_address: ComponentAddress,
            method_name: String,
            args: Vec<Bytes>,
        ) -> tari_bor::Value {
            ComponentManager::get(component_address).call(method_name, args)
        }

        pub fn call_component_with_args_using_proof(
            component_address: ComponentAddress,
            method_name: String,
            proof: Proof,
            amount: Amount,
        ) -> tari_bor::Value {
            ComponentManager::get(component_address).call(method_name, invoke_args![proof, amount])
        }
    }
}
