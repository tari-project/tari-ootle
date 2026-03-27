//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

// Initialize a StableCoin component from a template.
// Requires the template hash to be known at manifest authoring time.

use template_687b0d5b3bee2e987a72c0f8b0b9286968803eba9040ed67e3a85b8465ad294a as StableCoin;

fn fee_main() {
    let account = arg!["account"];
    account.pay_fee(2000);
}

fn main() {
    StableCoin::initialize(
        "100000000000000000000000000",
        "STC",
        Metadata("provider_name=StableCoin Inc."),
        8,
        PublicKey("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
        true,
    );
}
