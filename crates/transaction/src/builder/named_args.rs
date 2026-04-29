//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::error::Error;

use serde::Serialize;
use tari_bor::encode;

pub type BuilderWorkspaceKey = String;

pub struct ParsedBuilderWorkspaceKey {
    pub name: String,
    pub offset: Option<usize>,
}

/// Caller-supplied blob name that the builder will resolve to a `BlobIndex`.
pub type BuilderBlobKey = String;

/// The possible ways to represent an instruction's argument
#[derive(Debug, Clone, PartialEq)]
pub enum NamedArg {
    /// The argument is in the transaction execution's workspace, which means it is the result of a previous
    /// instruction
    Workspace(BuilderWorkspaceKey),
    /// The argument is a value specified in the transaction
    Literal(Vec<u8>),
    /// The argument is the contents of a transaction blob, referenced by name (which the
    /// builder resolves to a `BlobIndex` against blobs added via `add_blob`).
    Blob(BuilderBlobKey),
}

impl NamedArg {
    pub fn literal(value: tari_bor::Value) -> Result<Self, tari_bor::BorError> {
        Ok(Self::Literal(encode(&value)?))
    }

    pub fn from_type<T: Serialize>(val: &T) -> Result<Self, tari_bor::BorError> {
        Ok(Self::Literal(encode(val)?))
    }

    pub fn workspace<T: Into<BuilderWorkspaceKey>>(key: T) -> Self {
        Self::Workspace(key.into())
    }

    pub fn blob<T: Into<BuilderBlobKey>>(key: T) -> Self {
        Self::Blob(key.into())
    }

    pub fn as_literal(&self) -> Option<&[u8]> {
        match self {
            NamedArg::Literal(data) => Some(data),
            _ => None,
        }
    }
}

/// Trait for converting a value into a [`NamedArg`] for use in template method calls.
///
/// This enables macro-generated template methods to accept either:
/// - Concrete values (any `T: Serialize`, CBOR-encoded into `NamedArg::Literal`)
/// - Workspace references (`NamedArg::Workspace`, created via `workspace!("key")`)
pub trait IntoArg {
    fn into_arg(self) -> NamedArg;
}

impl IntoArg for NamedArg {
    fn into_arg(self) -> NamedArg {
        self
    }
}

impl<T: Serialize> IntoArg for T {
    /// Converts a serializable type to a `NamedArg::Literal` by CBOR-encoding it.
    ///
    /// ## Panics
    /// if CBOR serialization fails.
    fn into_arg(self) -> NamedArg {
        NamedArg::from_type(&self).expect("Failed to serialize argument into NamedArg")
    }
}

/// Utility macro for building a workspace argument.
/// ```rust,ignore
/// workspace!("foo") // expands to NamedArg::workspace("foo")
/// ```
#[macro_export]
macro_rules! workspace {
    ($name:expr) => {
        $crate::builder::named_args::NamedArg::workspace($name)
    };
}

/// Low-level macro used for counting characters in the encoding of arguments. Not intended for general usage
#[macro_export]
macro_rules! __expr_counter {
    () => (0usize);
    ( $x:expr $(,)? ) => (1usize);
    ( $x:expr, $($next:tt)* ) => (1usize + $crate::__expr_counter!($($next)*));
}

/// Utility macro for building a single instruction argument
#[macro_export]
macro_rules! arg {
    (Workspace($arg:expr)) => {
        $crate::builder::named_args::NamedArg::workspace($arg)
    };
    (Blob($arg:expr)) => {
        $crate::builder::named_args::NamedArg::blob($arg)
    };
    (Literal($arg:expr)) => {
        $crate::builder::named_args::NamedArg::from_type(&$arg).unwrap()
    };

    ($arg:expr) => {
        $crate::arg!(Literal($arg))
    };
}

/// Low-level macro for building instruction arguments, used by both `arg!` and `args!` macros. Not intended for general
/// usage.
#[macro_export]
macro_rules! __args_inner {
    (@ { $this:ident } Workspace($e:expr), $($tail:tt)*) => {
        $crate::builder::named_args::__push(&mut $this, $crate::arg!(Workspace($e)));
        $crate::__args_inner!(@ { $this } $($tail)*);
    };

    (@ { $this:ident } Workspace($e:expr) $(,)?) => {
        $crate::builder::named_args::__push(&mut $this, $crate::arg!(Workspace($e)));
    };

    (@ { $this:ident } Blob($e:expr), $($tail:tt)*) => {
        $crate::builder::named_args::__push(&mut $this, $crate::arg!(Blob($e)));
        $crate::__args_inner!(@ { $this } $($tail)*);
    };

    (@ { $this:ident } Blob($e:expr) $(,)?) => {
        $crate::builder::named_args::__push(&mut $this, $crate::arg!(Blob($e)));
    };

    (@ { $this:ident } Literal($e:expr), $($tail:tt)*) => {
        $crate::builder::named_args::__push(&mut $this, $crate::arg!(Literal($e)));
        $crate::__args_inner!(@ { $this } $($tail)*);
    };

    (@ { $this:ident } Literal($e:expr) $(,)?) => {
        $crate::builder::named_args::__push(&mut $this, $crate::arg!(Literal($e)));
    };

    (@ { $this:ident } $e:expr, $($tail:tt)*) => {
        $crate::builder::named_args::__push(&mut $this, $crate::arg!(Literal($e)));
        $crate::__args_inner!(@ { $this } $($tail)*);
    };

    (@ { $this:ident } $e:expr $(,)*) => {
        $crate::builder::named_args::__push(&mut $this, $crate::arg!(Literal($e)));
    };

    (@ { $this:ident } $(,)?) => { };
}

/// Utility macro for building multiple Ootle transaction instruction arguments.
///
/// Examples
/// ```ignore
/// let args = args![]; // Empty
/// let args = args![42, "bar"]; // literal arguments
///
/// // Workspace arguments are resolved at runtime.
/// let args = args![Workspace("foo"), 42, "bar"];
///
/// // Any data type that implements `serde::Serialize` can be used as a literal argument.
/// let args = args![MyStruct { field1: 42, field2: "hello".to_string() }];
/// ```
///
/// Note that some types may be "coerced" into the template function type during deserialization. For example, integer
/// literals may be deserialized into any compatible integer type (e.g., `u8`, `i32`, `u64`, `Amount`, etc.). The
/// mechanism for this varies but it usually simply that types have the same CBOR representation. However, Amount
/// type for instance, explicitly supports deserialization from integer/string/Amount byte literals.
#[macro_export]
macro_rules! args {
    () => (Vec::new());

    ($token:ident($args:expr), $($tail:tt)*) => {{
        let mut args = Vec::with_capacity(1 + $crate::__expr_counter!($($tail)*));
        $crate::__args_inner!(@ { args } $token($args), $($tail)*);
        args
    }};

    ($token:ident($args:expr) $(,)?) => {{
        let mut args = Vec::new();
        $crate::__args_inner!(@ { args } $token($args),);
        args
    }};

    ($args:expr, $($tail:tt)*) => {{
        let mut args = Vec::with_capacity(1 + $crate::__expr_counter!($($tail)*));
        $crate::__args_inner!(@ { args } Literal($args), $($tail)*);
        args
    }};

    ($args:expr $(,)?) => {{
        let mut args = Vec::new();
        $crate::__args_inner!(@ { args } Literal($args),);
        args
    }};
}

pub mod __macro_exports {
    pub use tari_template_lib_types::Amount;
}

// This is a workaround for a false positive for `clippy::vec_init_then_push` with this macro. We cannot ignore this
// lint as expression attrs are experimental.
#[allow(clippy::inline_always)]
#[inline(always)]
pub fn __push<T>(v: &mut Vec<T>, arg: T) {
    v.push(arg);
}

#[derive(Debug)]
pub enum ParseWorkspaceKeyError {
    EmptyKey,
    InvalidOffsetInteger,
}

impl Error for ParseWorkspaceKeyError {}

impl std::fmt::Display for ParseWorkspaceKeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Failed to parse workspace key {:?}", self)
    }
}

pub fn parse_workspace_key(key: BuilderWorkspaceKey) -> Result<ParsedBuilderWorkspaceKey, ParseWorkspaceKeyError> {
    if key.is_empty() {
        return Err(ParseWorkspaceKeyError::EmptyKey);
    }

    // Only support name.n format for now
    match key.split_once('.') {
        Some((name, offset)) => {
            let offset = offset
                .parse::<usize>()
                .map_err(|_| ParseWorkspaceKeyError::InvalidOffsetInteger)?;
            Ok(ParsedBuilderWorkspaceKey {
                name: name.to_string(),
                offset: Some(offset),
            })
        },
        None => Ok(ParsedBuilderWorkspaceKey {
            name: key,
            offset: None,
        }),
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn args_macro() {
        let args = args![Workspace("foo")];
        assert_eq!(args[0], NamedArg::workspace("foo"));

        let args = args!["foo".to_string()];
        assert!(matches!(args[0], NamedArg::Literal(_)));

        let args = args!["foo".to_string(), "bar".to_string(),];
        assert!(matches!(args[0], NamedArg::Literal(_)));
        assert!(matches!(args[1], NamedArg::Literal(_)));

        let args = args![Workspace("foo"), "bar".to_string()];
        assert_eq!(args[0], NamedArg::workspace("foo"));
        assert_eq!(
            args[1],
            NamedArg::literal(tari_bor::to_value(&"bar".to_string()).unwrap()).unwrap()
        );

        let args = args!["foo".to_string(), Workspace("bar"), 123u64];
        assert_eq!(
            args[0],
            NamedArg::literal(tari_bor::to_value(&"foo".to_string()).unwrap()).unwrap()
        );
        assert_eq!(args[1], NamedArg::workspace("bar"));
        assert_eq!(
            args[2],
            NamedArg::literal(tari_bor::to_value(&123u64).unwrap()).unwrap()
        );
    }

    #[test]
    fn args_macro_blob() {
        let args = args![Blob("template")];
        assert_eq!(args[0], NamedArg::blob("template"));

        let args = args![Workspace("ws"), Blob("data"), 42u64];
        assert_eq!(args[0], NamedArg::workspace("ws"));
        assert_eq!(args[1], NamedArg::blob("data"));
        assert_eq!(args[2], NamedArg::literal(tari_bor::to_value(&42u64).unwrap()).unwrap());
    }
}
