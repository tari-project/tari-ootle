//! Purity guard: the `ootle-sdk-core` crate must never pull a forbidden dependency into its tree.
//!
//! See `README.md`. If this test fails, a `tokio` / `reqwest` / `uniffi` / `cbindgen` /
//! `wasm-bindgen` crept into the dependency tree — back it out; the work belongs in a host or a
//! facade, not in `ootle-sdk-core`.

use std::process::Command;

const FORBIDDEN: &[&str] = &["tokio", "reqwest", "uniffi", "cbindgen", "wasm-bindgen"];

#[test]
fn dependency_tree_is_pure() {
    let output = Command::new(env!("CARGO"))
        .args(["tree", "-p", "ootle-sdk-core", "--edges", "normal,build"])
        .output()
        .expect("failed to run `cargo tree`");

    assert!(
        output.status.success(),
        "`cargo tree` failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let tree = String::from_utf8_lossy(&output.stdout);
    let offenders: Vec<&str> = FORBIDDEN
        .iter()
        .copied()
        .filter(|crate_name| {
            // `cargo tree` prefixes each node with Unicode box-drawing characters
            // (`├──`, `└──`, `│`), which `str::trim_start` does NOT strip, so we cannot
            // anchor with `starts_with`. Instead match the crate-node token `<name> v`
            // (name followed by a space and the version's leading `v`). The trailing
            // " v" guards against substring false-positives like `mini-tokio-validator`.
            let needle = format!("{crate_name} v");
            tree.lines().any(|line| line.contains(&needle))
        })
        .collect();

    assert!(
        offenders.is_empty(),
        "forbidden dependency(ies) {offenders:?} found in ootle-sdk-core tree:\n{tree}"
    );
}
