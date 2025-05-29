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

/// The possible ways to represent an instruction's argument
#[derive(Debug, Clone, PartialEq)]
pub enum NamedArg {
    /// The argument is in the transaction execution's workspace, which means it is the result of a previous
    /// instruction
    Workspace(BuilderWorkspaceKey),
    /// The argument is a value specified in the transaction
    Literal(Vec<u8>),
}

impl NamedArg {
    pub fn literal(value: tari_bor::Value) -> Result<Self, tari_bor::BorError> {
        // TODO: Unfortunately, CBOR value does not serialize consistently in JSON so we have to use the byte encoded
        // form for now.
        Ok(Self::Literal(encode(&value)?))
    }

    pub fn from_type<T: Serialize>(val: &T) -> Result<Self, tari_bor::BorError> {
        Ok(Self::Literal(encode(val)?))
    }

    pub fn workspace<T: Into<BuilderWorkspaceKey>>(key: T) -> Self {
        Self::Workspace(key.into())
    }

    pub fn as_literal_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Workspace(_) => None,
            Self::Literal(bytes) => Some(bytes),
        }
    }

    pub fn into_literal_bytes(self) -> Option<Vec<u8>> {
        match self {
            Self::Workspace(_) => None,
            Self::Literal(bytes) => Some(bytes),
        }
    }
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
    // Deprecated
    (Variable($arg:expr)) => {
        $crate::builder::named_args::NamedArg::workspace($arg)
    };
    (Workspace($arg:expr)) => {
        $crate::builder::named_args::NamedArg::workspace($arg)
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
    (@ { $this:ident } Variable($e:expr), $($tail:tt)*) => {
        $crate::builder::named_args::__push(&mut $this, $crate::arg!(Workspace($e)));
        $crate::__args_inner!(@ { $this } $($tail)*);
    };

    (@ { $this:ident } Variable($e:expr) $(,)?) => {
        $crate::builder::named_args::__push(&mut $this, $crate::arg!(Workspace($e)));
    };

    (@ { $this:ident } Workspace($e:expr), $($tail:tt)*) => {
        $crate::builder::named_args::__push(&mut $this, $crate::arg!(Workspace($e)));
        $crate::__args_inner!(@ { $this } $($tail)*);
    };

    (@ { $this:ident } Workspace($e:expr) $(,)?) => {
        $crate::builder::named_args::__push(&mut $this, $crate::arg!(Workspace($e)));
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

/// Utility macro for building multiple instruction arguments
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
}
