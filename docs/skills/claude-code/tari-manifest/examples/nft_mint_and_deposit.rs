//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

// Mint an NFT from a component and deposit it into an account.
// Variables: account (component address), nft_component (component address)

fn fee_main() {
    let account = var!["account"];
    account.pay_fee(1000);
}

fn main() {
    let account = var!["account"];
    let sparkle_nft = var!["nft_component"];

    // Mint a new NFT with a specific ID
    let nft_bucket = sparkle_nft.mint_specific(NonFungibleId("SpecialEdition"));

    // Deposit the minted NFT into the account
    account.deposit(nft_bucket);
}
