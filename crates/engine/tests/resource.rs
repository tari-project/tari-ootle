//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::{call_args, models::ComponentAddress};
use tari_template_test_tooling::{support::confidential::generate_confidential_output_statement, TemplateTest};

#[test]
fn fungible_join() {
    let mut test = TemplateTest::new(vec!["tests/templates/resource"]);
    let component: ComponentAddress = test.call_function("ResourceTest", "new", call_args![], vec![]);
    test.call_method::<()>(component, "fungible_join", call_args![], vec![]);
}

#[test]
fn non_fungible_join() {
    let mut test = TemplateTest::new(vec!["tests/templates/resource"]);
    let component: ComponentAddress = test.call_function("ResourceTest", "new", call_args![], vec![]);
    test.call_method::<()>(component, "non_fungible_join", call_args![], vec![]);
}

#[test]
fn confidential_join() {
    let mut test = TemplateTest::new(vec!["tests/templates/resource"]);
    let component: ComponentAddress = test.call_function("ResourceTest", "new", call_args![], vec![]);
    let (output, _, _) = generate_confidential_output_statement(1000, None);
    test.call_method::<()>(component, "confidential_join", call_args![output], vec![]);
}
