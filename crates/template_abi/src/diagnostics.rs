//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Markers for the panics raised by macro-generated dispatch code.
//!
//! A published template carries every string it can panic with: those bytes are replicated to every
//! validator and paid for at publish time. A dispatcher's decode failure names the function, the
//! argument position and the argument type, all of which the engine already holds in the
//! [`FunctionDef`](crate::FunctionDef) it is invoking. The template therefore panics with a short
//! marker and the engine renders the message from its own copy of the definition.
//!
//! The marker is produced by the template macro at expansion time, so a template carries the
//! resulting literal and none of this module's code.
//!
//! A message that carries no marker — a template author's own `panic!`, or a template published
//! before this encoding existed — is passed through untouched.
//!
//! A marker must stay legible on its own. A template published today runs on engines that predate
//! this module, which surface the marker verbatim as the reject reason, and a published template
//! cannot be amended once an engine is upgraded. The body therefore says which argument failed in
//! words rather than in a code, and only the function name and argument type — the parts an engine
//! can supply — are left out.

use crate::rust::{format, string::String};

/// Leading character of a marker. A control character, so an author's own panic text cannot collide
/// with it by accident.
pub const MARKER: char = '\u{1}';

/// Separates a marker from the underlying error the dispatcher formats after it.
const DETAIL_SEPARATOR: &str = ": ";

const COMPONENT_BODY: &str = "state decode";
const ARGUMENT_BODY_PREFIX: &str = "arg ";
const ARGUMENT_BODY_SUFFIX: &str = " decode";

/// A decode failure raised by macro-generated dispatch code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanicDiagnostic {
    /// The component instance — or, for a migration, the component address — could not be decoded.
    ComponentDecode,
    /// The argument at `index` in [`FunctionDef::arguments`](crate::FunctionDef::arguments) could
    /// not be decoded.
    ArgDecode { index: usize },
}

impl PanicDiagnostic {
    /// The marker for this diagnostic, without the detail that follows it.
    pub fn marker(&self) -> String {
        match *self {
            Self::ComponentDecode => format!("{MARKER}{COMPONENT_BODY}"),
            Self::ArgDecode { index } => format!("{MARKER}{ARGUMENT_BODY_PREFIX}{index}{ARGUMENT_BODY_SUFFIX}"),
        }
    }

    /// The `panic!` format string for this diagnostic: the marker followed by a single `{}` for the
    /// underlying decode error.
    pub fn panic_format_string(&self) -> String {
        format!("{}{DETAIL_SEPARATOR}{{}}", self.marker())
    }

    /// Splits a panic message into the diagnostic it marks and the detail that follows, or `None`
    /// for a message that carries no marker.
    pub fn parse(message: &str) -> Option<(Self, &str)> {
        let marked = message.strip_prefix(MARKER)?;
        let (body, detail) = marked.split_once(DETAIL_SEPARATOR).unwrap_or((marked, ""));
        if body == COMPONENT_BODY {
            return Some((Self::ComponentDecode, detail));
        }
        let index = body
            .strip_prefix(ARGUMENT_BODY_PREFIX)?
            .strip_suffix(ARGUMENT_BODY_SUFFIX)?
            .parse()
            .ok()?;
        Some((Self::ArgDecode { index }, detail))
    }
}

#[cfg(feature = "std")]
mod render {
    use super::{DETAIL_SEPARATOR, PanicDiagnostic};
    use crate::{
        FunctionDef,
        Type,
        rust::{format, string::String},
    };

    impl PanicDiagnostic {
        /// Renders the message the template would have carried, drawing the function name and the
        /// argument type from `func_def`.
        ///
        /// `None` when the diagnostic does not describe `func_def` — an index past its arguments —
        /// which means the marker did not come from this function's dispatcher.
        pub fn render(&self, func_def: &FunctionDef, detail: &str) -> Option<String> {
            let name = &func_def.name;
            match *self {
                Self::ComponentDecode => Some(format!(
                    "failed to decode component instance for function '{name}'{DETAIL_SEPARATOR}{detail}"
                )),
                Self::ArgDecode { index } => {
                    let arg = func_def.arguments.get(index)?;
                    let kind = match arg.arg_type {
                        Type::Tuple(_) => "tuple argument",
                        _ => "argument",
                    };
                    let arg_type = &arg.arg_type;
                    Some(format!(
                        "failed to decode {kind} at position {index} ({arg_type}) for function \
                         '{name}'{DETAIL_SEPARATOR}{detail}"
                    ))
                },
            }
        }
    }

    /// Expands `message` into the full diagnostic if it carries a marker, and returns it unchanged
    /// otherwise.
    pub fn expand_panic_message(func_def: &FunctionDef, message: String) -> String {
        let Some((diagnostic, detail)) = PanicDiagnostic::parse(&message) else {
            return message;
        };
        diagnostic.render(func_def, detail).unwrap_or(message)
    }
}

#[cfg(feature = "std")]
pub use render::expand_panic_message;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ArgDef, FunctionDef, Type};

    fn func_def() -> FunctionDef {
        FunctionDef {
            name: "withdraw".to_string(),
            arguments: vec![
                ArgDef {
                    name: "self".to_string(),
                    arg_type: Type::Other {
                        name: "&mut self".to_string(),
                    },
                },
                ArgDef {
                    name: "amount".to_string(),
                    arg_type: Type::Other {
                        name: "Amount".to_string(),
                    },
                },
                ArgDef {
                    name: "pair".to_string(),
                    arg_type: Type::Tuple(vec![Type::String, Type::U32]),
                },
            ],
            output: Type::Unit,
            is_mut: true,
            is_migration: false,
        }
    }

    #[test]
    fn markers_round_trip() {
        for diagnostic in [PanicDiagnostic::ComponentDecode, PanicDiagnostic::ArgDecode {
            index: 7,
        }] {
            let message = format!("{}: boom", diagnostic.marker());
            assert_eq!(PanicDiagnostic::parse(&message), Some((diagnostic, "boom")));
        }
    }

    #[test]
    fn format_string_takes_the_error_as_its_only_argument() {
        let format_string = PanicDiagnostic::ArgDecode { index: 2 }.panic_format_string();
        assert_eq!(format_string, "\u{1}arg 2 decode: {}");
    }

    #[test]
    fn a_marker_reads_as_a_message_on_its_own() {
        // What an engine without `expand_panic_message` surfaces as the reject reason.
        assert_eq!(
            format!("{}: unexpected type", PanicDiagnostic::ArgDecode { index: 1 }.marker()),
            "\u{1}arg 1 decode: unexpected type"
        );
        assert_eq!(
            format!("{}: end of input bytes", PanicDiagnostic::ComponentDecode.marker()),
            "\u{1}state decode: end of input bytes"
        );
    }

    #[test]
    fn a_message_without_a_marker_is_left_alone() {
        assert_eq!(PanicDiagnostic::parse("something went wrong"), None);
        assert_eq!(
            expand_panic_message(&func_def(), "something went wrong".to_string()),
            "something went wrong"
        );
    }

    #[test]
    fn renders_an_argument_failure() {
        let message = format!("{}: unexpected type", PanicDiagnostic::ArgDecode { index: 1 }.marker());
        assert_eq!(
            expand_panic_message(&func_def(), message),
            "failed to decode argument at position 1 (Amount) for function 'withdraw': unexpected type"
        );
    }

    #[test]
    fn renders_a_tuple_argument_failure() {
        let message = format!("{}: unexpected type", PanicDiagnostic::ArgDecode { index: 2 }.marker());
        assert_eq!(
            expand_panic_message(&func_def(), message),
            "failed to decode tuple argument at position 2 (Tuple<String,U32>) for function 'withdraw': unexpected \
             type"
        );
    }

    #[test]
    fn renders_a_component_failure() {
        let message = format!("{}: end of input bytes", PanicDiagnostic::ComponentDecode.marker());
        assert_eq!(
            expand_panic_message(&func_def(), message),
            "failed to decode component instance for function 'withdraw': end of input bytes"
        );
    }

    #[test]
    fn an_index_past_the_definition_is_passed_through() {
        let message = format!("{}: boom", PanicDiagnostic::ArgDecode { index: 9 }.marker());
        assert_eq!(expand_panic_message(&func_def(), message.clone()), message);
    }

    #[test]
    fn an_unknown_body_is_passed_through() {
        assert_eq!(PanicDiagnostic::parse("\u{1}something else: boom"), None);
        assert_eq!(PanicDiagnostic::parse("\u{1}arg two decode: boom"), None);
        assert_eq!(PanicDiagnostic::parse("\u{1}state decode 4: boom"), None);
    }

    /// Shapes a template could panic with to try to derail the parse. The message is chosen by the
    /// executing WASM, so this is attacker-controlled input at a consensus boundary: every one of
    /// these must land on a rendered message or verbatim pass-through, never a panic.
    const CRAFTED: &[&str] = &[
        "",
        "\u{1}",
        "\u{1}: ",
        "\u{1}arg",
        "\u{1}arg ",
        "\u{1}arg  decode: x",
        "\u{1}arg decode: x",
        "\u{1}arg 1 decode",
        "\u{1}arg 1 decode:",
        "\u{1}arg 1 decode: ",
        "\u{1}arg 1  decode: x",
        "\u{1}arg 01 decode: x",
        "\u{1}arg +1 decode: x",
        "\u{1}arg -1 decode: x",
        "\u{1}arg 1.0 decode: x",
        // usize::MAX, and one digit past it
        "\u{1}arg 18446744073709551615 decode: x",
        "\u{1}arg 18446744073709551616 decode: x",
        "\u{1}arg 999999999999999999999999999999 decode: x",
        "\u{1}state decode",
        "\u{1}state decode: ",
        "\u{1}state decode 1: x",
        "\u{1}state  decode: x",
        // Multi-byte sequences either side of every boundary the parse splits on
        "\u{1}\u{1F600} decode: x",
        "\u{1}arg \u{1F600} decode: x",
        "\u{1}arg 1 decode: \u{1F600}",
        "\u{1}arg 1 decode\u{1F600}: x",
        "\u{1F600}arg 1 decode: x",
        // Separators and markers embedded in the detail
        "\u{1}arg 1 decode: a: b",
        "\u{1}arg 1 decode: \u{1}state decode: nested",
        "\u{1}arg 1 decode: \u{1}",
        "\u{1}\u{1}arg 1 decode: x",
        // Text that reads like a rendered message, with and without the marker
        "failed to decode argument at position 0 (Amount) for function 'x': forged",
        "\u{1}failed to decode argument at position 0 (Amount) for function 'x': forged",
    ];

    #[test]
    fn crafted_messages_render_or_pass_through() {
        let func_def = func_def();
        for message in CRAFTED {
            let expanded = expand_panic_message(&func_def, message.to_string());
            match PanicDiagnostic::parse(message) {
                Some((PanicDiagnostic::ArgDecode { index }, _)) if index >= func_def.arguments.len() => {
                    assert_eq!(&expanded, message, "out-of-range index rewrote {message:?}");
                },
                Some(_) => assert!(
                    expanded.starts_with("failed to decode"),
                    "{message:?} rendered as {expanded:?}"
                ),
                None => assert_eq!(&expanded, message, "unparsed message rewrote {message:?}"),
            }
        }
    }

    #[test]
    fn random_messages_render_or_pass_through() {
        // A deterministic hammer over the alphabet the parse actually branches on. Cheap enough to
        // run on every build; `fuzz/fuzz_targets/panic_diagnostic_expand.rs` covers the same sink
        // with real coverage guidance.
        const ALPHABET: [&str; 12] = [
            "\u{1}",
            "arg",
            "state",
            "decode",
            " ",
            ":",
            ": ",
            "0",
            "9",
            "\u{1F600}",
            "",
            "-",
        ];
        let func_def = func_def();
        let mut seed = 0x5eed_1234_u64;
        for _ in 0..20_000 {
            let mut message = String::new();
            let mut len = 0;
            while len < 8 {
                // xorshift64
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                message.push_str(ALPHABET[(seed % ALPHABET.len() as u64) as usize]);
                len += 1;
            }
            let expanded = expand_panic_message(&func_def, message.clone());
            assert!(
                expanded == message || expanded.starts_with("failed to decode"),
                "{message:?} expanded to {expanded:?}"
            );
        }
    }

    #[test]
    fn an_author_message_shaped_like_a_body_is_passed_through() {
        // Without the marker there is nothing to expand, however closely the text matches.
        let message = "arg 1 decode: mine".to_string();
        assert_eq!(PanicDiagnostic::parse(&message), None);
        assert_eq!(expand_panic_message(&func_def(), message.clone()), message);
    }

    #[test]
    fn a_marker_without_detail_renders_with_none() {
        let message = PanicDiagnostic::ComponentDecode.marker();
        assert_eq!(
            expand_panic_message(&func_def(), message),
            "failed to decode component instance for function 'withdraw': "
        );
    }
}
