//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_transaction::{Transaction, args};
use tari_template_lib::types::constants::{STEALTH_TARI_RESOURCE_ADDRESS, XTR_FAUCET_COMPONENT_ADDRESS};
use tari_template_test_tooling::{TemplateTest, support::assert_error::assert_reject_reason};

/// A brand-new signer can call take() and receive tokens.
#[test]
fn first_claim_succeeds() {
    let mut test = TemplateTest::new_builtin_only();

    // create_funded_account calls faucet.take() internally and commits the result
    let (account, _proof, _sk) = test.create_funded_account();

    let balance: tari_template_lib::types::Amount =
        test.call_method(account, "balance", args![STEALTH_TARI_RESOURCE_ADDRESS], vec![]);
    assert!(balance > tari_template_lib::types::Amount::zero());
}

/// Two different signers can each claim once independently — they have different public keys,
/// so their claim-receipt NFT IDs are different.
#[test]
fn different_signers_can_each_claim_once() {
    let mut test = TemplateTest::new_builtin_only();
    // Each call uses a fresh key pair (auto-incrementing seed inside create_funded_account)
    let _unused = test.create_funded_account();
    let _unused = test.create_funded_account();
    // If we reach here without panic, both claims succeeded
}

/// The same signer cannot call take() twice.
/// The second attempt is rejected with "Duplicate NFT token id".
#[test]
fn second_claim_by_same_signer_is_rejected() {
    let mut test = TemplateTest::new_builtin_only();
    // First claim: succeeds and commits state (including the claim-receipt burned NFT)
    let (account, _owner_proof, secret_key) = test.create_funded_account();

    // Second claim: same signing key → same claim-receipt NFT ID → DuplicateNonFungibleId
    let reject_reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            .call_method(XTR_FAUCET_COMPONENT_ADDRESS, "take", args![account])
            .build_and_seal(&secret_key),
        vec![],
    );
    assert_reject_reason(&reject_reason, "Duplicate NFT token id");
}
