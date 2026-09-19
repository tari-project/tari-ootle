//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! A transaction declares each input as a read or a write. A shard group that does not hold an input
//! locks it from that declaration alone, without executing, so the declaration has to be an upper
//! bound on access rather than a hint: writing to a read-declared input aborts here.

use tari_engine::runtime::RuntimeError;
use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::InputDeclaration;
use tari_ootle_transaction::{Epoch, Transaction, args};
use tari_template_lib::types::ComponentAddress;
use tari_template_test_tooling::{TemplateTest, support::assert_error::assert_reject_reason};

const CRATE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"));

fn setup() -> (TemplateTest, ComponentAddress) {
    let mut test = TemplateTest::new(CRATE_PATH, vec!["tests/templates/state"]);
    let component: ComponentAddress = test.call_function("State", "new", args![], vec![]);
    (test, component)
}

#[test]
fn a_read_declared_input_may_be_read() {
    let (mut test, component) = setup();

    let value: u32 = test.call_method(component, "get", args![], vec![]);
    assert_eq!(value, 0);

    let result = test.execute_expect_success(
        Transaction::builder_localnet(Epoch(1))
            .add_input(InputDeclaration::read(component))
            .call_method(component, "get", args![])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    assert!(result.finalize.is_accept());
}

#[test]
fn writing_to_a_read_declared_input_aborts() {
    let (mut test, component) = setup();

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet(Epoch(1))
            .add_input(InputDeclaration::read(component))
            .call_method(component, "set", args![42u32])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    assert_reject_reason(&reason, RuntimeError::WriteToReadDeclaredInput {
        id: SubstateId::Component(component),
    });

    // The abort is what stops the write, so the component still holds its original value.
    let value: u32 = test.call_method(component, "get", args![], vec![]);
    assert_eq!(value, 0);
}

#[test]
fn a_write_declared_input_may_be_written() {
    let (mut test, component) = setup();

    test.execute_expect_success(
        Transaction::builder_localnet(Epoch(1))
            .add_input(InputDeclaration::write(component))
            .call_method(component, "set", args![42u32])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    let value: u32 = test.call_method(component, "get", args![], vec![]);
    assert_eq!(value, 42);
}

#[test]
fn a_substate_declared_both_ways_is_a_write() {
    let (mut test, component) = setup();

    // `add_input` widens rather than letting declaration order decide, in either order.
    for (first, second) in [
        (InputDeclaration::read(component), InputDeclaration::write(component)),
        (InputDeclaration::write(component), InputDeclaration::read(component)),
    ] {
        let transaction = Transaction::builder_localnet(Epoch(1))
            .add_input(first)
            .add_input(second)
            .call_method(component, "set", args![7u32])
            .build_and_seal(test.secret_key());

        assert_eq!(transaction.inputs().len(), 1);
        test.execute_expect_success(transaction, vec![]);
    }

    let value: u32 = test.call_method(component, "get", args![], vec![]);
    assert_eq!(value, 7);
}
