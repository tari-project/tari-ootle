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
//! An engine without [`expand_panic_message`] surfaces the marker verbatim, so a decode failure
//! there reads `\u{1}A2: <error>` rather than a sentence: the tag, the argument position and the
//! underlying error, without the function name or argument type. Worth knowing when reading a
//! reject reason from an older validator.

use crate::rust::{format, string::String};

/// Leading character of a marker. A control character, so an author's own panic text cannot collide
/// with it by accident.
pub const MARKER: char = '\u{1}';

/// Separates a marker from the underlying error the dispatcher formats after it.
const DETAIL_SEPARATOR: &str = ": ";

const TAG_COMPONENT: char = 'C';
const TAG_ARGUMENT: char = 'A';

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
            Self::ComponentDecode => format!("{MARKER}{TAG_COMPONENT}"),
            Self::ArgDecode { index } => format!("{MARKER}{TAG_ARGUMENT}{index}"),
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
        let mut chars = body.chars();
        let tag = chars.next()?;
        let rest = chars.as_str();
        match tag {
            // The tag carries no field, so trailing bytes mean this is not a marker we wrote.
            TAG_COMPONENT if rest.is_empty() => Some((Self::ComponentDecode, detail)),
            TAG_ARGUMENT => Some((
                Self::ArgDecode {
                    index: rest.parse().ok()?,
                },
                detail,
            )),
            _ => None,
        }
    }
}

#[cfg(feature = "std")]
mod render {
    use core::fmt::{self, Write};

    use super::{DETAIL_SEPARATOR, PanicDiagnostic};
    use crate::{FunctionDef, Type, rust::string::String};

    /// Ends a rendered message cut short at its length limit.
    pub const TRUNCATION_SUFFIX: &str = "…";

    /// Keeps what is written to it up to `max_len` bytes, cut at a char boundary, and fails the write that reaches
    /// past them so that whatever is rendering stops there.
    struct BoundedWriter {
        buf: String,
        max_len: usize,
        truncated: bool,
    }

    impl Write for BoundedWriter {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            let room = self.max_len - self.buf.len();
            if s.len() <= room {
                self.buf.push_str(s);
                return Ok(());
            }
            self.buf.push_str(&s[..s.floor_char_boundary(room)]);
            self.truncated = true;
            Err(fmt::Error)
        }
    }

    /// `args` rendered to at most `max_len` bytes. A rendering that does not fit is cut short and ends with
    /// [`TRUNCATION_SUFFIX`], and the rest of it is never rendered.
    fn render_bounded(args: fmt::Arguments<'_>, max_len: usize) -> String {
        let mut writer = BoundedWriter {
            buf: String::new(),
            max_len,
            truncated: false,
        };
        // A failed write is the writer reaching its limit, recorded as `truncated`.
        let _ = writer.write_fmt(args);
        let mut rendered = writer.buf;
        if writer.truncated && TRUNCATION_SUFFIX.len() <= max_len {
            rendered.truncate(rendered.floor_char_boundary(max_len - TRUNCATION_SUFFIX.len()));
            rendered.push_str(TRUNCATION_SUFFIX);
        }
        rendered
    }

    impl PanicDiagnostic {
        /// Renders the message the template would have carried, drawing the function name and the
        /// argument type from `func_def`, to at most `max_len` bytes. A declared argument type can
        /// render to far more than the template's own message, so a message that does not fit is cut
        /// short and ends with [`TRUNCATION_SUFFIX`].
        ///
        /// `None` when the diagnostic does not describe `func_def`, which means the marker did not
        /// come from this function's dispatcher: an index past its parameters, or index 0 on a
        /// method or migration, whose component the dispatcher reports as `ComponentDecode`.
        pub fn render(&self, func_def: &FunctionDef, detail: &str, max_len: usize) -> Option<String> {
            let name = &func_def.name;
            match *self {
                Self::ComponentDecode => Some(render_bounded(
                    format_args!("failed to decode component instance for function '{name}'{DETAIL_SEPARATOR}{detail}"),
                    max_len,
                )),
                Self::ArgDecode { index } => {
                    // The dispatcher counts the component among a method's or migration's parameters, and
                    // `arguments` does not.
                    let position = if func_def.takes_component() {
                        index.checked_sub(1)?
                    } else {
                        index
                    };
                    let arg = func_def.arguments.get(position)?;
                    let kind = match arg.arg_type {
                        Type::Tuple(_) => "tuple argument",
                        _ => "argument",
                    };
                    let arg_type = &arg.arg_type;
                    Some(render_bounded(
                        format_args!(
                            "failed to decode {kind} at position {index} ({arg_type}) for function \
                             '{name}'{DETAIL_SEPARATOR}{detail}"
                        ),
                        max_len,
                    ))
                },
            }
        }
    }

    /// Expands `message` into the full diagnostic, of at most `max_len` bytes, if it carries a
    /// marker, and returns it unchanged otherwise.
    pub fn expand_panic_message(func_def: &FunctionDef, message: String, max_len: usize) -> String {
        let Some((diagnostic, detail)) = PanicDiagnostic::parse(&message) else {
            return message;
        };
        diagnostic.render(func_def, detail, max_len).unwrap_or(message)
    }
}

#[cfg(feature = "std")]
pub use render::{TRUNCATION_SUFFIX, expand_panic_message};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ArgDef, FunctionDef, FunctionKind, Receiver, Type};

    /// The bound the engine renders a panic message to (`ENGINE_LIMITS.max_panic_message_size`).
    const LIMIT: usize = 32 * 1024;

    fn func_def() -> FunctionDef {
        FunctionDef {
            name: "withdraw".to_string(),
            kind: FunctionKind::Method(Receiver::RefMut),
            arguments: vec![
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
        assert_eq!(format_string, "\u{1}A2: {}");
    }

    #[test]
    fn a_marker_is_what_an_engine_without_the_renderer_shows() {
        assert_eq!(
            format!("{}: unexpected type", PanicDiagnostic::ArgDecode { index: 1 }.marker()),
            "\u{1}A1: unexpected type"
        );
        assert_eq!(
            format!("{}: end of input bytes", PanicDiagnostic::ComponentDecode.marker()),
            "\u{1}C: end of input bytes"
        );
    }

    #[test]
    fn a_message_without_a_marker_is_left_alone() {
        assert_eq!(PanicDiagnostic::parse("something went wrong"), None);
        assert_eq!(
            expand_panic_message(&func_def(), "something went wrong".to_string(), LIMIT),
            "something went wrong"
        );
    }

    #[test]
    fn renders_an_argument_failure() {
        let message = format!("{}: unexpected type", PanicDiagnostic::ArgDecode { index: 1 }.marker());
        assert_eq!(
            expand_panic_message(&func_def(), message, LIMIT),
            "failed to decode argument at position 1 (Amount) for function 'withdraw': unexpected type"
        );
    }

    #[test]
    fn renders_a_tuple_argument_failure() {
        let message = format!("{}: unexpected type", PanicDiagnostic::ArgDecode { index: 2 }.marker());
        assert_eq!(
            expand_panic_message(&func_def(), message, LIMIT),
            "failed to decode tuple argument at position 2 (Tuple<String,U32>) for function 'withdraw': unexpected \
             type"
        );
    }

    #[test]
    fn renders_a_migration_argument_failure_past_the_previous_state() {
        let migration = FunctionDef {
            name: "migrate".to_string(),
            kind: FunctionKind::Migration {
                from: Type::Other {
                    name: "CounterV1".to_string(),
                },
            },
            ..func_def()
        };
        let message = format!("{}: unexpected type", PanicDiagnostic::ArgDecode { index: 1 }.marker());
        assert_eq!(
            expand_panic_message(&migration, message, LIMIT),
            "failed to decode argument at position 1 (Amount) for function 'migrate': unexpected type"
        );
    }

    #[test]
    fn renders_a_component_failure() {
        let message = format!("{}: end of input bytes", PanicDiagnostic::ComponentDecode.marker());
        assert_eq!(
            expand_panic_message(&func_def(), message, LIMIT),
            "failed to decode component instance for function 'withdraw': end of input bytes"
        );
    }

    #[test]
    fn an_index_past_the_definition_is_passed_through() {
        let message = format!("{}: boom", PanicDiagnostic::ArgDecode { index: 9 }.marker());
        assert_eq!(expand_panic_message(&func_def(), message.clone(), LIMIT), message);
    }

    #[test]
    fn an_unknown_tag_is_passed_through() {
        assert_eq!(PanicDiagnostic::parse("\u{1}Z1: boom"), None);
        assert_eq!(PanicDiagnostic::parse("\u{1}C4: boom"), None);
        assert_eq!(PanicDiagnostic::parse("\u{1}Atwo: boom"), None);
    }

    /// Shapes a template could panic with to try to derail the parse. The message is chosen by the
    /// executing WASM, so this is attacker-controlled input at a consensus boundary: every one of
    /// these must land on a rendered message or verbatim pass-through, never a panic.
    const CRAFTED: &[&str] = &[
        "",
        "\u{1}",
        "\u{1}: ",
        "\u{1}A",
        "\u{1}A: x",
        "\u{1}A 1: x",
        "\u{1}A01: x",
        "\u{1}A+1: x",
        "\u{1}A-1: x",
        "\u{1}A1.0: x",
        "\u{1}A1",
        "\u{1}A1:",
        "\u{1}A1: ",
        // usize::MAX, and one digit past it
        "\u{1}A18446744073709551615: x",
        "\u{1}A18446744073709551616: x",
        "\u{1}A999999999999999999999999999999: x",
        "\u{1}C",
        "\u{1}C: ",
        "\u{1}C4: x",
        "\u{1}c: x",
        "\u{1}Z1: x",
        // Multi-byte sequences either side of every boundary the parse splits on
        "\u{1}\u{1F600}: x",
        "\u{1}A\u{1F600}: x",
        "\u{1}A1: \u{1F600}",
        "\u{1}A1\u{1F600}: x",
        "\u{1F600}A1: x",
        // Separators and markers embedded in the detail
        "\u{1}A1: a: b",
        "\u{1}A1: \u{1}C: nested",
        "\u{1}A1: \u{1}",
        "\u{1}\u{1}A1: x",
        // Text that reads like a rendered message, with and without the marker
        "failed to decode argument at position 0 (Amount) for function 'x': forged",
        "\u{1}failed to decode argument at position 0 (Amount) for function 'x': forged",
    ];

    #[test]
    fn crafted_messages_render_or_pass_through() {
        let func_def = func_def();
        for message in CRAFTED {
            let expanded = expand_panic_message(&func_def, message.to_string(), LIMIT);
            match PanicDiagnostic::parse(message) {
                Some((PanicDiagnostic::ArgDecode { index }, _))
                    if (index == 0 && func_def.takes_component()) || index >= func_def.num_call_args() =>
                {
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
            let expanded = expand_panic_message(&func_def, message.clone(), LIMIT);
            assert!(
                expanded == message || expanded.starts_with("failed to decode"),
                "{message:?} expanded to {expanded:?}"
            );
        }
    }

    fn func_def_taking(arg_type: Type) -> FunctionDef {
        FunctionDef {
            name: "withdraw".to_string(),
            kind: FunctionKind::Function,
            arguments: vec![ArgDef {
                name: "arg".to_string(),
                arg_type,
            }],
            output: Type::Unit,
        }
    }

    /// The declared argument type comes from the template, and can render to far more than the message the template
    /// panicked with.
    #[test]
    fn a_rendering_past_the_limit_is_cut_short() {
        let message = format!("{}: x", PanicDiagnostic::ArgDecode { index: 0 }.marker());
        let wide = Type::Tuple(vec![Type::Tuple(vec![Type::String; 32]); 32]);
        let long_name = Type::Vec(Box::new(Type::Other {
            name: "\u{1F600}".repeat(LIMIT),
        }));
        for arg_type in [wide, long_name] {
            let expanded = expand_panic_message(&func_def_taking(arg_type), message.clone(), 1024);
            assert!(expanded.len() <= 1024, "rendered {} bytes", expanded.len());
            assert!(expanded.starts_with("failed to decode"), "{expanded:.80}");
            assert!(expanded.ends_with(TRUNCATION_SUFFIX), "{expanded:.80}");
        }
    }

    #[test]
    fn a_rendering_within_the_limit_is_whole() {
        let message = format!("{}: unexpected type", PanicDiagnostic::ArgDecode { index: 2 }.marker());
        let whole = expand_panic_message(&func_def(), message.clone(), LIMIT);
        assert_eq!(expand_panic_message(&func_def(), message, whole.len()), whole);
    }

    #[test]
    fn an_author_message_shaped_like_a_marker_is_passed_through() {
        // Without the marker there is nothing to expand, however closely the text matches.
        let message = "A1: mine".to_string();
        assert_eq!(PanicDiagnostic::parse(&message), None);
        assert_eq!(expand_panic_message(&func_def(), message.clone(), LIMIT), message);
    }

    #[test]
    fn a_marker_without_detail_renders_with_none() {
        let message = PanicDiagnostic::ComponentDecode.marker();
        assert_eq!(
            expand_panic_message(&func_def(), message, LIMIT),
            "failed to decode component instance for function 'withdraw': "
        );
    }
}
