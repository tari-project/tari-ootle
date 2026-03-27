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

#[template]
mod caller_context_template {
    use super::*;

    pub struct CallerContextTest {
        caller_pub_key: RistrettoPublicKeyBytes,
    }

    impl CallerContextTest {
        pub fn log_caller_pk() {
            let caller_pub_key = CallerContext::transaction_signer_public_key();
            info!("{}", caller_pub_key);
        }

        pub fn main_signer_proof() -> Proof {
            CallerContext::get_main_signer_proof()
        }

        pub fn signer_proof_for_pk(public_key: RistrettoPublicKeyBytes) -> Proof {
            CallerContext::get_signer_proof_for_public_key(public_key)
        }

        pub fn check_pk_proof(proof: Proof, expected_pk: RistrettoPublicKeyBytes) {
            assert!(proof.resource_address().is_public_key_resource());
            proof.authorize_with(|| {});
            let badge = proof.get_non_fungibles().pop_first().unwrap();
            assert_eq!(badge.as_u256().unwrap(), expected_pk.as_bytes());
        }

        /// Calls the `main_signer_proof` function of the template at the given address, using the `TemplateManager` to
        /// call across template contexts. All signers are out of scope in cross-template calls
        pub fn call_using_cross_template(addr: TemplateAddress) -> Proof {
            TemplateManager::get(addr).call("main_signer_proof", args![])
        }
    }
}
