//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::ToByteType;
use tari_ootle_address::OotleAddress;

use crate::support::Test;

mod support;

#[test]
fn it_generates_new_account_addresses() {
    let test = Test::new();
    let account_addr = test.sdk().key_manager_api().next_account_address().unwrap();
    let addr = account_addr.address.to_byte_type().to_bech32_string();

    let decoded = OotleAddress::decode_bech32(&addr).unwrap();

    assert_eq!(decoded.network(), test.sdk().network());
    assert_eq!(
        *decoded.account_public_key(),
        account_addr.address.account_key.to_byte_type()
    );
    assert_eq!(
        *decoded.view_only_key(),
        account_addr.address.view_only_key().to_byte_type()
    );

    let account_addr2 = test.sdk().key_manager_api().next_account_address().unwrap();
    assert_ne!(account_addr.address, account_addr2.address);
}
