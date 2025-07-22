//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_wallet_crypto::confidential;
use tari_template_lib::types::Amount;

#[test]
fn it_create_a_valid_revealed_only_proof() {
    let proof =
        confidential::create_withdraw_proof(&[], Amount::from(123), None, Amount::from(123), None, Amount::from(0))
            .unwrap();

    assert!(proof.is_revealed_only());
}
