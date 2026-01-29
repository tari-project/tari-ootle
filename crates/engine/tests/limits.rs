//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_bor::encoded_len;
use tari_engine::wasm::WasmExecutionError;
use tari_engine_types::limits;
use tari_ootle_transaction::{args, Transaction};
use tari_template_abi::CallInfoRef;
use tari_template_lib::types::bytes::Bytes;
use tari_template_test_tooling::{support::assert_error::assert_reject_reason, TemplateTest};

const TEMPLATE_PATHS: &[&str] = &["tests/templates/limits"];
const TEMPLATE_NAME: &str = "PushItToTheLimit";
const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");

#[test]
fn max_call_size_limit() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);
    let template = test.get_template_address(TEMPLATE_NAME);
    let max_bytes = Bytes::from(vec![123u8; limits::ENGINE_LIMITS.max_call_size]);
    let value = tari_bor::to_value(&max_bytes).unwrap();
    let call_size = encoded_len(&CallInfoRef {
        func_name: "new",
        args: &[value],
    })
    .unwrap();
    let overhead = call_size - limits::ENGINE_LIMITS.max_call_size;

    test.execute_expect_success(
        Transaction::builder_localnet()
            .call_function(
                template,
                "new",
                args!(Bytes::from(vec![123u8; limits::ENGINE_LIMITS.max_call_size - overhead])),
            )
            .build_and_seal(test.secret_key()),
        vec![],
    );

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            .call_function(
                template,
                "new",
                args!(Bytes::from(vec![123u8; limits::ENGINE_LIMITS.max_call_size])),
            )
            .build_and_seal(test.secret_key()),
        vec![],
    );

    assert_reject_reason(reason, WasmExecutionError::CallSizeLimitExceeded {
        limit: limits::ENGINE_LIMITS.max_call_size,
    });
}

// TODO: add tests for other limits
