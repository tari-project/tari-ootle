//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use cucumber::then;
use integration_tests::TariWorld;

#[then(expr = "I check that outputs {word} contain {int} unspent outputs")]
async fn when_i_wait_for_validator_leaf_block_at_least(world: &mut TariWorld, name: String, num_outputs: u64) {
    let outputs = world.outputs.get(&name).expect("Output not found");
    let output_count = outputs.values().filter(|o| o.substate_id().is_utxo()).count();
    assert_eq!(
        output_count as u64, num_outputs,
        "Number of unspent outputs does not match"
    );
}
