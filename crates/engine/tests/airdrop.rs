//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ootle_byte_type::ToByteType;
use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::substate_type::SubstateType;
use tari_ootle_transaction::{args, call_args, Transaction};
use tari_template_lib::{models::ComponentAddress, types::Amount};
use tari_template_test_tooling::TemplateTest;

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");

fn setup() -> (TemplateTest, ComponentAddress, SubstateId) {
    let mut template_test = TemplateTest::new(CRATE_PATH, vec!["tests/templates/nft/airdrop"]);
    let airdrop: ComponentAddress = template_test.call_function("Airdrop", "new", call_args![], vec![]);
    let airdrop_resx = template_test.get_previous_output_address(SubstateType::Resource);
    (template_test, airdrop, airdrop_resx)
}

#[test]
fn airdrop() {
    let (mut test, airdrop, airdrop_resx) = setup();

    let total_supply: Amount = test.call_method(airdrop, "total_supply", call_args![], vec![test.owner_proof()]);
    assert_eq!(total_supply, Amount::from(100));

    let builder = Transaction::builder_localnet().then(|builder| {
        // Create 50 accounts
        (0..50).fold(builder, |builder, _| {
            let (_, owner_public_key, _) = test.create_owner_proof();
            builder.create_account(owner_public_key.to_byte_type())
        })
    });

    test.build_and_execute(builder, vec![test.owner_proof()])
        .unwrap_success();

    let addresses = test
        .read_only_state_store()
        .all_accounts()
        .unwrap()
        .into_keys()
        .collect::<Vec<_>>();

    test.call_method::<()>(airdrop, "open_airdrop", call_args![], vec![test.owner_proof()]);

    test.build_and_execute(
        Transaction::builder_localnet().then(|builder| {
            addresses.iter().fold(builder, |builder, addr| {
                builder.call_method(airdrop, "add_recipient", args![addr])
            })
        }),
        vec![test.owner_proof()],
    )
    .unwrap_success();

    let result = test.build_and_execute(
        Transaction::builder_localnet().then(|builder| {
            addresses.iter().fold(builder, |builder, addr| {
                builder
                    .call_method(airdrop, "claim_any", args![addr])
                    .put_last_instruction_output_on_workspace("claimed")
                    .call_method(*addr, "deposit", args![Workspace("claimed")])
                    .call_method(*addr, "balance", args![airdrop_resx.as_resource_address().unwrap()])
            })
        }),
        vec![test.owner_proof()],
    );
    result.expect_success();

    for i in 0..50 {
        assert_eq!(
            result.finalize.execution_results[3 + (i * 4)]
                .decode::<Amount>()
                .unwrap(),
            1
        );
    }
}
