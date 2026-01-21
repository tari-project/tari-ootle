//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_transaction::{args, Transaction};
use tari_template_lib::{prelude::ComponentAddress, types::Amount};
use tari_template_test_tooling::{support::confidential::generate_confidential_output_statement, TemplateTest};

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");

#[test]
fn it_burns_all_resource_types() {
    let mut test = TemplateTest::new(CRATE_PATH, ["tests/templates/burn"]);
    let recall_template = test.get_template_address("Burn");

    let (mut initial_supply, _mask, _) = generate_confidential_output_statement(1000, None);
    initial_supply.output_revealed_amount = Amount::from(1000);

    let result = test.execute_expect_success(
        Transaction::builder_localnet()
            .call_function(recall_template, "new", args![initial_supply])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    let component: ComponentAddress = result.finalize.execution_results[0].decode().unwrap();

    let fungible_vault = test.extract_component_value(component, "$.fungible");
    let non_fungible_vault = test.extract_component_value(component, "$.non_fungible");
    let confidential_vault = test.extract_component_value(component, "$.confidential");
    let stealth_vault = test.extract_component_value(component, "$.stealth");

    {
        let store = test.read_only_state_store();
        let fungible_resource = *store.get_vault(&fungible_vault).unwrap().resource_address();
        let resource = store.get_resource(&fungible_resource).unwrap();
        assert_eq!(resource.total_supply(), Some(Amount::from(1_000_000)));

        let non_fungible_resource = *store.get_vault(&non_fungible_vault).unwrap().resource_address();
        let resource = store.get_resource(&non_fungible_resource).unwrap();
        assert_eq!(resource.total_supply(), Some(Amount::from(10)));

        let confidential_resource = *store.get_vault(&confidential_vault).unwrap().resource_address();
        let resource = store.get_resource(&confidential_resource).unwrap();
        assert_eq!(resource.total_supply(), Some(Amount::from(1000)));

        let stealth_resource = *store.get_vault(&stealth_vault).unwrap().resource_address();
        let resource = store.get_resource(&stealth_resource).unwrap();
        assert_eq!(resource.total_supply(), Some(Amount::from(1_000_000)));
    }

    test.execute_expect_success(
        Transaction::builder_localnet()
            .call_method(component, "burn_all", args![])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    let vault = test.read_only_state_store().get_vault(&fungible_vault).unwrap();
    assert_eq!(vault.balance(), 0);

    let vault = test.read_only_state_store().get_vault(&non_fungible_vault).unwrap();
    assert_eq!(vault.balance(), 0);

    let vault = test.read_only_state_store().get_vault(&confidential_vault).unwrap();
    assert_eq!(vault.balance(), 0);

    let vault = test.read_only_state_store().get_vault(&stealth_vault).unwrap();
    assert_eq!(vault.balance(), 0);

    {
        let store = test.read_only_state_store();
        let fungible_resource = *store.get_vault(&fungible_vault).unwrap().resource_address();
        let resource = store.get_resource(&fungible_resource).unwrap();
        assert_eq!(resource.total_supply(), Some(Amount::ZERO));

        let non_fungible_resource = *store.get_vault(&non_fungible_vault).unwrap().resource_address();
        let resource = store.get_resource(&non_fungible_resource).unwrap();
        assert_eq!(resource.total_supply(), Some(Amount::ZERO));

        let confidential_resource = *store.get_vault(&confidential_vault).unwrap().resource_address();
        let resource = store.get_resource(&confidential_resource).unwrap();
        assert_eq!(resource.total_supply(), Some(Amount::ZERO));

        let stealth_resource = *store.get_vault(&stealth_vault).unwrap().resource_address();
        let resource = store.get_resource(&stealth_resource).unwrap();
        assert_eq!(resource.total_supply(), Some(Amount::ZERO));
    }
}
