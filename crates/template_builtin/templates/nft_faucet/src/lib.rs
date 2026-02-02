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

use tari_template_lib::{prelude::*, types::constants::NFT_FAUCET_RESOURCE_ADDRESS};

#[template]
mod template {

    use super::*;

    pub struct NftFaucet {
        serial_number: u64,
    }

    impl NftFaucet {
        pub fn mint(&mut self, amount: Amount, mutable_data: tari_bor::Value) -> Bucket {
            if amount.is_zero() || amount.is_negative() {
                panic!("Amount must be greater than zero");
            }
            if amount >= Amount::from(10u64) {
                panic!("Amount must be less than or equal to 10");
            }

            let owner = CallerContext::transaction_signer_public_key().to_string();

            let mut metadata = Metadata::new();
            metadata.insert("original_minter", &owner);

            let mut counter = 0;
            let amount_to_mint = amount.to_u64_checked().expect("Amount must be a positive");
            let manager = ResourceManager::get(NFT_FAUCET_RESOURCE_ADDRESS);
            manager.mint_many_non_fungible_with(&metadata, &mutable_data, || {
                if counter == amount_to_mint {
                    return None;
                }
                let id = NonFungibleId::from_u64(self.serial_number);
                counter += 1;
                self.serial_number += 1;
                Some(id)
            })
        }
    }
}
