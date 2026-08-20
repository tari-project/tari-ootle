//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#![no_main]
//! Panic-message expansion — `tari_template_abi::diagnostics::expand_panic_message`.
//!
//! The message is chosen by the template being executed, so it is attacker-controlled at a
//! consensus boundary: `WasmProcess::invoke` feeds whatever the WASM passed to the `on_panic` host
//! call straight into the expander, capped only at `ENGINE_LIMITS.max_panic_message_size` (32 KiB)
//! and truncated to a char boundary. A panic in the expander would take down the executing
//! validator, so every crafted shape has to land on either a rendered message or verbatim
//! pass-through.
//!
//! Invariants asserted, beyond "does not panic":
//!   * a message with no marker is returned byte-for-byte;
//!   * output is either the input verbatim or a rendered diagnostic;
//!   * an index past the definition's arguments never renders;
//!   * output is bounded by the input plus the definition's own text, so a crafted message cannot amplify allocation.

use libfuzzer_sys::fuzz_target;
use tari_template_abi::{
    diagnostics::{expand_panic_message, PanicDiagnostic, MARKER},
    ArgDef,
    FunctionDef,
    Type,
};

/// Longest text `render` can add from the definition itself: the fixed wording, plus the function
/// name and argument type built below.
const MAX_RENDER_OVERHEAD: usize = 256;

fn func_def(arg_count: u8) -> FunctionDef {
    let arguments = (0..arg_count)
        .map(|i| ArgDef {
            name: format!("arg_{i}"),
            arg_type: match i % 4 {
                0 => Type::Other {
                    name: "Amount".to_string(),
                },
                1 => Type::Tuple(vec![Type::String, Type::U32]),
                2 => Type::Vec(Box::new(Type::U8)),
                _ => Type::Unit,
            },
        })
        .collect();

    FunctionDef {
        name: "withdraw".to_string(),
        arguments,
        output: Type::Unit,
        is_mut: true,
        is_migration: false,
    }
}

fuzz_target!(|data: (&str, u8)| {
    let (message, arg_count) = data;
    let func_def = func_def(arg_count);

    let expanded = expand_panic_message(&func_def, message.to_string());

    let parsed = PanicDiagnostic::parse(message);

    // A message the dispatcher did not write is diagnostic text in its own right and must survive
    // untouched — this is what carries an author's `panic!` to the reject reason.
    if !message.starts_with(MARKER) {
        assert_eq!(parsed, None, "marker-less message parsed: {message:?}");
        assert_eq!(expanded, message, "marker-less message rewritten: {message:?}");
        return;
    }

    let renders = match parsed {
        None => false,
        Some((PanicDiagnostic::ComponentDecode, _)) => true,
        // Out of range means the marker did not come from this function's dispatcher.
        Some((PanicDiagnostic::ArgDecode { index }, _)) => index < func_def.arguments.len(),
    };

    if renders {
        assert!(
            expanded.starts_with("failed to decode"),
            "rendered message has an unexpected shape: {expanded:?}"
        );
        assert!(
            expanded.len() <= message.len() + MAX_RENDER_OVERHEAD,
            "render amplified {} bytes into {}",
            message.len(),
            expanded.len()
        );
    } else {
        assert_eq!(expanded, message, "unrenderable marker rewritten: {message:?}");
    }
});
