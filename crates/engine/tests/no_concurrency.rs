//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_transaction::{args, Transaction};
use tari_template_test_tooling::{support::assert_error::assert_reject_reason, TemplateTest};

const TEMPLATE_NAME: &str = "NoConcurrency";
const TEMPLATE_PATHS: &[&str] = &["no_concurrency"];
const CRATE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/templates");

#[test]
fn it_panics_if_template_attempts_to_spawn_a_thread() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            .call_function(
                test.get_template_address(TEMPLATE_NAME),
                "try_to_spawn_a_thread_static",
                args![],
            )
            .finish()
            .seal(test.secret_key()),
        vec![],
    );

    // Source of the error message:
    // https://github.com/rust-lang/rust/blob/3fda0e426ca17b0baa0d6e765a0d23f487350573/library/std/src/sys/thread/unsupported.rs#L20
    // This means that the thread spawn was not even attempted from WASM.

    // We have explicitly disabled threads in the engine and not included wasmer-wasix which would provide threads
    // support. Currently, thread support is not included in the lastest wasi_preview2 spec, but wasmer have attempted
    // to support it with wasmer-wasix as a stop-gap extension to the wasi API.
    //
    // WASI is not a new WASM instruction set, and the way thread spawning would require a host function to spawn the
    // thread. Therefore, this test doesn't really make sense. If a user successfully compiles a
    // template with "thread support" and attempts to run it in the engine, the instantiation would fail due to missing
    // host functions. Importantly, we validate that the binary does not require invalid imports before
    // accepting the template on chain.
    assert_reject_reason(&reason, "operation not supported on this platform");
}
