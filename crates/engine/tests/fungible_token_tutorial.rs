//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Worked examples for the "Fungible Tokens (ERC-20)" guide
//! (`docs/developer-docs/src/content/docs/guides/fungible-tokens.mdx`).
//!
//! The guide embeds the code below and the `fungible_token` template verbatim, so these tests are what keeps it
//! honest. [`guide_snippets_match_their_source`] checks that the page and its sources have not drifted apart.

use std::{fs, path::Path};

use tari_ootle_transaction::args;
use tari_template_lib::{
    prelude::{ComponentAddress, ResourceAddress, TARI_TOKEN},
    types::{Amount, metadata},
};
use tari_template_test_tooling::{TemplateTest, support::assert_error::assert_reject_reason};

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");

fn token_in(test: &TemplateTest, account: ComponentAddress) -> (ResourceAddress, Amount) {
    let vaults = test.read_only_state_store().get_vaults_for_account(account).unwrap();
    vaults
        .iter()
        .find(|(resource, _)| **resource != TARI_TOKEN)
        .map(|(resource, vault)| (*resource, vault.balance()))
        .expect("account holds no token")
}

#[test]
fn fixed_supply_token() {
    let mut test = TemplateTest::new(CRATE_PATH, ["tests/templates/fungible_token"]);
    let template = test.get_template_address("FungibleToken");
    let (alice, _alice_proof, alice_key) = test.create_empty_account();

    // The whole supply is created in one go and deposited into the creator's account.
    test.execute_expect_success(
        test.transaction()
            .call_function(template, "create_fixed_supply", args![
                "EXT",
                6u8,
                Amount::from(21_000_000_000_000u64),
                metadata!["name" => "Example Token"],
            ])
            .put_last_instruction_output_on_workspace("supply")
            .call_method(alice, "deposit", args![Workspace("supply")])
            .build_and_seal(&alice_key),
        vec![],
    );

    let (token, balance) = token_in(&test, alice);
    assert_eq!(balance, Amount::from(21_000_000_000_000u64));

    let resource = test.read_only_state_store().get_resource(&token).unwrap();
    assert_eq!(resource.total_supply(), Some(balance));
    assert_eq!(resource.divisibility(), 6);
    assert_eq!(resource.metadata().get_str("name"), Some("Example Token"));
}

#[test]
fn mint_transfer_and_burn() {
    let mut test = TemplateTest::new(CRATE_PATH, ["tests/templates/fungible_token"]);
    let template = test.get_template_address("FungibleToken");
    let (alice, _alice_proof, alice_key) = test.create_empty_account();
    let (bob, _bob_proof, bob_key) = test.create_empty_account();

    // Alice creates the token and mints the first batch into Alice's account in one transaction. The signer of
    // the transaction that creates the component owns it, so Alice may call `mint`.
    test.execute_expect_success(
        test.transaction()
            .call_function(template, "create_mintable", args![
                "EXT",
                6u8,
                metadata!["name" => "Example Token"]
            ])
            .put_last_instruction_output_on_workspace("token")
            .call_method("token", "mint", args![Amount::from(1_000_000_000u64)])
            .put_last_instruction_output_on_workspace("minted")
            .call_method(alice, "deposit", args![Workspace("minted")])
            .build_and_seal(&alice_key),
        vec![],
    );
    let (minter, _) = test
        .read_only_state_store()
        .get_components_by_template_address(template)
        .unwrap()
        .remove(0);
    let (token, _) = token_in(&test, alice);

    // ERC-20 `transfer`: withdraw a bucket from the sender's account and deposit it into the receiver's.
    test.execute_expect_success(
        test.transaction()
            .call_method(alice, "withdraw", args![token, Amount::from(250_000_000u64)])
            .put_last_instruction_output_on_workspace("payment")
            .call_method(bob, "deposit", args![Workspace("payment")])
            .build_and_seal(&alice_key),
        vec![],
    );
    assert_eq!(token_in(&test, bob).1, Amount::from(250_000_000u64));

    // Bob does not own the minter component, so `mint` is denied.
    let reason = test.execute_expect_failure(
        test.transaction()
            .call_method(minter, "mint", args![Amount::from(1u64)])
            .put_last_instruction_output_on_workspace("minted")
            .call_method(bob, "deposit", args![Workspace("minted")])
            .build_and_seal(&bob_key),
        vec![],
    );
    assert_reject_reason(reason, "Access Denied");

    // Alice burns tokens through the minter, which is the only place the burn rule allows it.
    test.execute_expect_success(
        test.transaction()
            .call_method(alice, "withdraw", args![token, Amount::from(100_000_000u64)])
            .put_last_instruction_output_on_workspace("to_burn")
            .call_method(minter, "burn", args![Workspace("to_burn")])
            .build_and_seal(&alice_key),
        vec![],
    );
    let resource = test.read_only_state_store().get_resource(&token).unwrap();
    assert_eq!(resource.total_supply(), Some(Amount::from(900_000_000u64)));
}

#[test]
fn approve_and_transfer_from() {
    let mut test = TemplateTest::new(CRATE_PATH, ["tests/templates/fungible_token"]);
    let template = test.get_template_address("FungibleToken");
    let (alice, _alice_proof, alice_key) = test.create_empty_account();
    // `bob_badge` is the public-key badge of Bob's signing key.
    let (bob, bob_badge, bob_key) = test.create_empty_account();

    test.execute_expect_success(
        test.transaction()
            .call_function(template, "create_fixed_supply", args![
                "EXT",
                6u8,
                Amount::from(1_000_000u64),
                metadata!["name" => "Example Token"],
            ])
            .put_last_instruction_output_on_workspace("supply")
            .call_method(alice, "deposit", args![Workspace("supply")])
            .build_and_seal(&alice_key),
        vec![],
    );
    let (token, _) = token_in(&test, alice);

    // ERC-20 `approve`: Alice lets Bob's key withdraw up to 400 units. `expected_current` is 0 for a first grant.
    test.execute_expect_success(
        test.transaction()
            .call_method(alice, "approve", args![
                bob_badge.clone(),
                token,
                Amount::zero(),
                Amount::from(400u64)
            ])
            .build_and_seal(&alice_key),
        vec![],
    );

    // ERC-20 `transferFrom`: Bob signs this transaction and spends part of the allowance.
    test.execute_expect_success(
        test.transaction()
            .call_method(alice, "withdraw_approved", args![
                bob_badge.clone(),
                token,
                Amount::from(300u64)
            ])
            .put_last_instruction_output_on_workspace("pulled")
            .call_method(bob, "deposit", args![Workspace("pulled")])
            .build_and_seal(&bob_key),
        vec![],
    );
    assert_eq!(token_in(&test, bob).1, Amount::from(300u64));

    // The remaining allowance is 100, so a second pull of 300 is rejected.
    let reason = test.execute_expect_failure(
        test.transaction()
            .call_method(alice, "withdraw_approved", args![
                bob_badge,
                token,
                Amount::from(300u64)
            ])
            .put_last_instruction_output_on_workspace("pulled")
            .call_method(bob, "deposit", args![Workspace("pulled")])
            .build_and_seal(&bob_key),
        vec![],
    );
    assert_reject_reason(reason, "Amount exceeds approval");
}

/// Every Rust block in the guide that carries a `title` path must appear verbatim in that file, ignoring
/// indentation, so that an example cannot silently drift away from the code this test suite compiles and runs.
#[test]
fn guide_snippets_match_their_source() {
    let repo_root = Path::new(CRATE_PATH).join("../..");
    let guide_path = repo_root.join("docs/developer-docs/src/content/docs/guides/fungible-tokens.mdx");
    let guide = fs::read_to_string(&guide_path).expect("guide is missing");

    let mut checked = 0;
    for (title, snippet) in rust_blocks_with_a_source_path(&guide) {
        let source = fs::read_to_string(repo_root.join(&title)).unwrap_or_else(|e| panic!("{title}: {e}"));
        assert!(
            contains_lines(&source, &snippet),
            "{}: a snippet in the guide is not present in {title} verbatim:\n{snippet}",
            guide_path.display()
        );
        checked += 1;
    }
    assert!(checked > 0, "no source-backed Rust blocks found in the guide");
}

fn rust_blocks_with_a_source_path(guide: &str) -> Vec<(String, String)> {
    let mut blocks = Vec::new();
    let mut lines = guide.lines();
    while let Some(line) = lines.next() {
        let Some(meta) = line.strip_prefix("```rust ") else {
            continue;
        };
        let Some(title) = meta.split_once("title=\"").and_then(|(_, t)| t.split_once('"')) else {
            continue;
        };
        let contents = lines
            .by_ref()
            .take_while(|l| !l.starts_with("```"))
            .collect::<Vec<_>>()
            .join("\n");
        blocks.push((title.0.to_string(), contents));
    }
    blocks
}

/// True if the non-blank lines of `snippet`, trimmed, appear consecutively in `source`.
fn contains_lines(source: &str, snippet: &str) -> bool {
    let trimmed = |s: &str| {
        s.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    let source = trimmed(source);
    let snippet = trimmed(snippet);
    !snippet.is_empty() && source.windows(snippet.len()).any(|w| w == snippet)
}
