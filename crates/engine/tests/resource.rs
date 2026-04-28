//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_transaction::args;
use tari_template_lib::types::{ComponentAddress, Metadata, ResourceAddress};
use tari_template_test_tooling::{TemplateTest, support::confidential::generate_confidential_output_statement};

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");

#[test]
fn fungible_join() {
    let mut test = TemplateTest::new(CRATE_PATH, vec!["tests/templates/resource"]);
    let component: ComponentAddress = test.call_function("ResourceTest", "new", args![], vec![]);
    test.call_method::<()>(component, "fungible_join", args![], vec![]);
}

#[test]
fn non_fungible_join() {
    let mut test = TemplateTest::new(CRATE_PATH, vec!["tests/templates/resource"]);
    let component: ComponentAddress = test.call_function("ResourceTest", "new", args![], vec![]);
    test.call_method::<()>(component, "non_fungible_join", args![], vec![]);
}

#[test]
fn confidential_join() {
    let mut test = TemplateTest::new(CRATE_PATH, vec!["tests/templates/resource"]);
    let component: ComponentAddress = test.call_function("ResourceTest", "new", args![], vec![]);
    let (output, _, _) = generate_confidential_output_statement(1000, None);
    test.call_method::<()>(component, "confidential_join", args![output], vec![]);
}

#[test]
fn update_metadata_succeeds() {
    let mut test = TemplateTest::new(CRATE_PATH, vec!["tests/templates/resource", "tests/templates/metadata"]);
    let component: ComponentAddress = test.call_function("MetadataTest", "new_with_symbol", args![], vec![]);
    let resource_address: ResourceAddress = test.call_method(component, "resource_address", args![], vec![]);

    let mut new_metadata = Metadata::new();
    new_metadata.insert("SYMBOL", "FOO");
    new_metadata.insert("description", "A fine token");
    test.call_method::<()>(component, "set_metadata", args![new_metadata], vec![]);

    let resource = test.read_only_state_store().get_resource(&resource_address).unwrap();
    assert_eq!(resource.metadata().get("description"), Some("A fine token"));
    assert_eq!(resource.metadata().get("SYMBOL"), Some("FOO"));
}

#[test]
fn update_metadata_rejects_symbol_change() {
    let mut test = TemplateTest::new(CRATE_PATH, vec!["tests/templates/resource", "tests/templates/metadata"]);
    let component: ComponentAddress = test.call_function("MetadataTest", "new_with_symbol", args![], vec![]);

    let mut new_metadata = Metadata::new();
    new_metadata.insert("SYMBOL", "BAR");
    let key = test.secret_key().clone();
    let reason = test.execute_expect_failure(
        test.transaction()
            .call_method(component, "set_metadata", args![new_metadata])
            .build_and_seal(&key),
        vec![],
    );
    assert!(
        reason.to_string().to_lowercase().contains("token symbol"),
        "expected symbol-immutability reason, got: {reason}"
    );
}

#[test]
fn update_metadata_rejects_symbol_drop() {
    let mut test = TemplateTest::new(CRATE_PATH, vec!["tests/templates/resource", "tests/templates/metadata"]);
    let component: ComponentAddress = test.call_function("MetadataTest", "new_with_symbol", args![], vec![]);

    let mut new_metadata = Metadata::new();
    new_metadata.insert("description", "symbol-less");
    let key = test.secret_key().clone();
    let reason = test.execute_expect_failure(
        test.transaction()
            .call_method(component, "set_metadata", args![new_metadata])
            .build_and_seal(&key),
        vec![],
    );
    assert!(
        reason.to_string().to_lowercase().contains("token symbol"),
        "expected symbol-immutability reason, got: {reason}"
    );
}

#[test]
fn create_rejects_oversized_token_symbol() {
    let mut test = TemplateTest::new(CRATE_PATH, vec!["tests/templates/resource", "tests/templates/metadata"]);
    let key = test.secret_key().clone();
    // 11 bytes — one over the 10-byte limit
    let symbol = "OVERSIZEDSY".to_string();
    let reason = test.execute_expect_failure(
        test.transaction()
            .call_function(
                test.get_template_address("MetadataTest"),
                "new_with_custom_symbol",
                args![symbol],
            )
            .build_and_seal(&key),
        vec![],
    );
    assert!(
        reason.to_string().to_lowercase().contains("token symbol"),
        "expected token-symbol-length reason, got: {reason}"
    );
}

#[test]
fn update_metadata_rejects_oversized_token_symbol() {
    let mut test = TemplateTest::new(CRATE_PATH, vec!["tests/templates/resource", "tests/templates/metadata"]);
    let component: ComponentAddress = test.call_function("MetadataTest", "new_without_symbol", args![], vec![]);

    let mut new_metadata = Metadata::new();
    // 11 bytes — one over the 10-byte limit
    new_metadata.insert("SYMBOL", "OVERSIZEDSY");
    let key = test.secret_key().clone();
    let reason = test.execute_expect_failure(
        test.transaction()
            .call_method(component, "set_metadata", args![new_metadata])
            .build_and_seal(&key),
        vec![],
    );
    assert!(
        reason.to_string().to_lowercase().contains("token symbol"),
        "expected token-symbol-length reason, got: {reason}"
    );
}

#[test]
fn update_metadata_can_set_symbol_when_not_previously_set() {
    let mut test = TemplateTest::new(CRATE_PATH, vec!["tests/templates/resource", "tests/templates/metadata"]);
    let component: ComponentAddress = test.call_function("MetadataTest", "new_without_symbol", args![], vec![]);
    let resource_address: ResourceAddress = test.call_method(component, "resource_address", args![], vec![]);

    let mut first = Metadata::new();
    first.insert("SYMBOL", "NEW");
    test.call_method::<()>(component, "set_metadata", args![first], vec![]);

    let resource = test.read_only_state_store().get_resource(&resource_address).unwrap();
    assert_eq!(resource.metadata().get("SYMBOL"), Some("NEW"));

    // Now it's set, further changes must be rejected.
    let mut second = Metadata::new();
    second.insert("SYMBOL", "CHANGED");
    let key = test.secret_key().clone();
    let reason = test.execute_expect_failure(
        test.transaction()
            .call_method(component, "set_metadata", args![second])
            .build_and_seal(&key),
        vec![],
    );
    assert!(
        reason.to_string().to_lowercase().contains("token symbol"),
        "expected symbol-immutability reason, got: {reason}"
    );
}
