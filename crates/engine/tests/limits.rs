//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::slice;

use tari_engine::{runtime::LimitError, wasm::WasmExecutionError};
use tari_engine_types::limits;
use tari_ootle_transaction::{Transaction, args};
use tari_template_abi::CallInfo;
use tari_template_lib::types::bytes::Bytes;
use tari_template_test_tooling::{TemplateTest, support::assert_error::assert_reject_reason};

const TEMPLATE_PATHS: &[&str] = &["tests/templates/limits"];
const TEMPLATE_NAME: &str = "PushItToTheLimit";
const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");

#[test]
fn max_call_size_limit() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);
    let template = test.get_template_address(TEMPLATE_NAME);
    let max_bytes = Bytes::from(vec![123u8; limits::ENGINE_LIMITS.max_call_size]);
    let value = tari_bor::to_value(&max_bytes).unwrap();
    let call_size = CallInfo::encode_v1_packed_size(slice::from_ref(&value)).unwrap();
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

#[test]
fn max_random_bytes_len_limit() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);
    let template = test.get_template_address(TEMPLATE_NAME);

    let max_len = limits::ENGINE_LIMITS.max_random_bytes_len as u32;

    let bytes: Vec<u8> = test.call_function(TEMPLATE_NAME, "request_random_bytes", args!(max_len), vec![]);
    assert_eq!(bytes.len(), max_len as usize);

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            .call_function(template, "request_random_bytes", args!(max_len + 1))
            .build_and_seal(test.secret_key()),
        vec![],
    );

    assert_reject_reason(reason, LimitError::MaxRandomBytesLenExceeded {
        len: (max_len + 1) as usize,
    });
}

// TODO: add tests for other limits
