//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::{substate::SubstateId, ToByteType};
use tari_ootle_common_types::substate_type::SubstateType;
use tari_template_lib::{call_args, models::ComponentAddress, types::Amount};
use tari_template_test_tooling::TemplateTest;
use tari_transaction::{args, Transaction};

fn setup() -> (TemplateTest, ComponentAddress, SubstateId) {
    let mut template_test = TemplateTest::new(vec!["tests/templates/nft/airdrop"]);
    let airdrop: ComponentAddress = template_test.call_function("Airdrop", "new", call_args![], vec![]);
    let airdrop_resx = template_test.get_previous_output_address(SubstateType::Resource);
    (template_test, airdrop, airdrop_resx)
}

#[test]
fn airdrop() {
    let (mut template_test, airdrop, airdrop_resx) = setup();

    let total_supply: Amount =
        template_test.call_method(airdrop, "total_supply", call_args![], vec![template_test.owner_proof()]);
    assert_eq!(total_supply, Amount::from(100));

    let builder = Transaction::builder().then(|builder| {
        // Create 100 accounts
        (0..100).fold(builder, |builder, _| {
            let (_, owner_public_key, _) = template_test.create_owner_proof();
            builder.create_account(owner_public_key.to_byte_type())
        })
    });

    let result = template_test
        .build_and_execute(builder, vec![template_test.owner_proof()])
        .unwrap_success();

    let addresses = result
        .finalize
        .execution_results
        .iter()
        .map(|r| r.decode::<ComponentAddress>().unwrap())
        .collect::<Vec<_>>();

    template_test.call_method::<()>(airdrop, "open_airdrop", call_args![], vec![template_test.owner_proof()]);

    template_test
        .build_and_execute(
            Transaction::builder().then(|builder| {
                addresses.iter().fold(builder, |builder, addr| {
                    builder.call_method(airdrop, "add_recipient", args![addr])
                })
            }),
            vec![template_test.owner_proof()],
        )
        .unwrap_success();

    let result = template_test.build_and_execute(
        Transaction::builder().then(|builder| {
            addresses.iter().fold(builder, |builder, addr| {
                builder
                    .call_method(airdrop, "claim_any", args![addr])
                    .put_last_instruction_output_on_workspace("claimed")
                    .call_method(*addr, "deposit", args![Workspace("claimed")])
                    .call_method(*addr, "balance", args![airdrop_resx.as_resource_address().unwrap()])
            })
        }),
        vec![template_test.owner_proof()],
    );
    result.expect_success();

    for i in 0..100 {
        assert_eq!(
            result.finalize.execution_results[3 + (i * 4)]
                .decode::<Amount>()
                .unwrap(),
            1
        );
    }
}
