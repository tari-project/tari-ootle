//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt;

use serde::{Deserialize, Serialize};
use tari_bor::encode;
use tari_template_lib::prelude::Bytes;

pub type WorkspaceId = u16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct WorkspaceOffsetId {
    id: WorkspaceId,
    offset: Option<usize>,
}

impl WorkspaceOffsetId {
    pub fn new(id: WorkspaceId) -> Self {
        Self { id, offset: None }
    }

    pub fn with_offset(self, offset: usize) -> Self {
        Self {
            id: self.id,
            offset: Some(offset),
        }
    }

    pub fn with_offset_opt(self, offset: Option<usize>) -> Self {
        Self { id: self.id, offset }
    }

    /// The workspace ID
    pub fn id(&self) -> WorkspaceId {
        self.id
    }

    /// The offset within the workspace, if provided. Offset refers to the index of an array or field/map entry
    /// within a workspace item.
    pub fn offset(&self) -> Option<usize> {
        self.offset
    }
}

impl fmt::Display for WorkspaceOffsetId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(offset) = self.offset {
            write!(f, "{}.{}", self.id, offset)
        } else {
            write!(f, "{}", self.id)
        }
    }
}

/// Represents an argument that can be passed to a transaction instruction. Either a literal value or a reference to a
/// item on the runtime's workspace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum InstructionArg {
    /// The argument is in the transaction execution's workspace, which means it is the result of a previous
    /// instruction
    Workspace(WorkspaceOffsetId),
    /// The argument is a value specified in the transaction
    Literal(
        #[serde(
            serialize_with = "ootle_serde::hex::serialize",
            deserialize_with = "ootle_serde::hex::deserialize_from_vec"
        )]
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        Bytes,
    ),
    // Literal(tari_bor::Value),
}

impl InstructionArg {
    pub fn literal(value: tari_bor::Value) -> Result<Self, tari_bor::BorError> {
        // TODO: Unfortunately, CBOR value does not serialize consistently in JSON so we have to use the byte encoded
        // form for now.
        Ok(Self::raw_literal_bytes(encode(&value)?))
    }

    pub fn raw_literal_bytes<T: Into<Bytes>>(bytes: T) -> Self {
        Self::Literal(bytes.into())
    }

    pub fn from_type<T: Serialize>(val: &T) -> Result<Self, tari_bor::BorError> {
        Ok(Self::raw_literal_bytes(encode(val)?))
    }

    pub fn workspace(id: WorkspaceId, offset: Option<usize>) -> Self {
        Self::workspace_offset(WorkspaceOffsetId::new(id).with_offset_opt(offset))
    }

    pub fn workspace_offset(id: WorkspaceOffsetId) -> Self {
        Self::Workspace(id)
    }

    pub fn as_literal_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Literal(bytes) => Some(bytes),
            Self::Workspace(_) => None,
        }
    }
}

/// Low-level macro for building instruction arguments, used by both `arg!` and `args!` macros. Not intended for general
/// usage.
#[macro_export]
macro_rules! __call_args_inner {
    (@ { $this:ident } Workspace($e:expr), $($tail:tt)*) => {
        $crate::args::__push(&mut $this, $crate::call_arg!(Workspace($e)));
        $crate::__call_args_inner!(@ { $this } $($tail)*);
    };

    (@ { $this:ident } Workspace($e:expr) $(,)?) => {
        $crate::args::__push(&mut $this, $crate::call_arg!(Workspace($e)));
    };

    (@ { $this:ident } WorkspaceOffset($e:expr), $($tail:tt)*) => {
        $crate::args::__push(&mut $this, $crate::call_arg!(WorkspaceOffset($e)));
        $crate::__call_args_inner!(@ { $this } $($tail)*);
    };

    (@ { $this:ident } WorkspaceOffset($e:expr) $(,)?) => {
        $crate::args::__push(&mut $this, $crate::call_arg!(WorkspaceOffset($e)));
    };

    (@ { $this:ident } Literal($e:expr), $($tail:tt)*) => {
        $crate::args::__push(&mut $this, $crate::call_arg!(Literal($e)));
        $crate::__call_args_inner!(@ { $this } $($tail)*);
    };

    (@ { $this:ident } Literal($e:expr) $(,)?) => {
        $crate::args::__push(&mut $this, $crate::call_arg!(Literal($e)));
    };

    (@ { $this:ident } $e:expr, $($tail:tt)*) => {
        $crate::args::__push(&mut $this, $crate::call_arg!(Literal($e)));
        $crate::__call_args_inner!(@ { $this } $($tail)*);
    };

    (@ { $this:ident } $e:expr $(,)*) => {
        $crate::args::__push(&mut $this, $crate::call_arg!(Literal($e)));
    };

    (@ { $this:ident } $(,)?) => { };
}

// This is a workaround for a false positive for `clippy::vec_init_then_push` with this macro. We cannot ignore this
// lint as expression attrs are experimental.
#[allow(clippy::inline_always)]
#[inline(always)]
pub fn __push<T>(v: &mut Vec<T>, arg: T) {
    v.push(arg);
}

/// Utility macro for building a single instruction argument
#[macro_export]
macro_rules! call_arg {
    (Workspace($id:expr, $offset:expr)) => {
        $crate::args::InstructionArg::workspace($id, $offset)
    };
    (WorkspaceOffset($offset_id:expr)) => {
        $crate::args::InstructionArg::workspace_offset($offset_id)
    };
    (Workspace($id:expr)) => {
        $crate::args::InstructionArg::workspace($id, None)
    };
    (Literal($arg:expr)) => {
        $crate::args::InstructionArg::from_type(&$arg).unwrap()
    };
    (Amount($arg:expr)) => {
        $crate::args::InstructionArg::from_type(&$crate::types::Amount::from($arg)).unwrap()
    };

    ($arg:expr) => {
        $crate::call_arg!(Literal($arg))
    };
}

/// Utility macro for building multiple raw instruction arguments either from literal values or workspace references
///
/// Examples:
/// ```ignore
/// #use tari_ootle_transaction::args::{call_args, InstructionArg};
/// let args = call_args![Workspace(1), "literal value", 42u64];
/// assert_eq!(args[0], InstructionArg::workspace(1, None));
/// assert!(matches!(args[1], InstructionArg::Literal(_)));
/// assert!(matches!(args[2], InstructionArg::Literal(_)));
/// ```
#[macro_export]
macro_rules! call_args {
    () => (Vec::new());

    ($token:ident($args:expr), $($tail:tt)*) => {{
        let mut args = Vec::with_capacity(1 + $crate::__expr_counter!($($tail)*));
        $crate::__call_args_inner!(@ { args } $token($args), $($tail)*);
        args
    }};

    ($token:ident($args:expr) $(,)?) => {{
        let mut args = Vec::new();
        $crate::__call_args_inner!(@ { args } $token($args),);
        args
    }};

    ($args:expr, $($tail:tt)*) => {{
        let mut args = Vec::with_capacity(1 + $crate::__expr_counter!($($tail)*));
        $crate::__call_args_inner!(@ { args } Literal($args), $($tail)*);
        args
    }};

    ($args:expr $(,)?) => {{
        let mut args = Vec::new();
        $crate::__call_args_inner!(@ { args } Literal($args),);
        args
    }};
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn args_macro() {
        let args = call_args![Workspace(1)];
        assert_eq!(args[0], InstructionArg::workspace(1, None));

        let args = call_args!["foo".to_string()];
        assert!(matches!(args[0], InstructionArg::Literal(_)));

        let args = call_args!["foo".to_string(), "bar".to_string(),];
        assert!(matches!(args[0], InstructionArg::Literal(_)));
        assert!(matches!(args[1], InstructionArg::Literal(_)));

        let args = call_args![Workspace(2), "bar".to_string()];
        assert_eq!(args[0], InstructionArg::workspace(2, None));
        assert_eq!(
            args[1],
            InstructionArg::literal(tari_bor::to_value(&"bar".to_string()).unwrap()).unwrap()
        );

        let args = call_args!["foo".to_string(), Workspace(3), 123u64];
        assert_eq!(
            args[0],
            InstructionArg::literal(tari_bor::to_value(&"foo".to_string()).unwrap()).unwrap()
        );
        assert_eq!(args[1], InstructionArg::workspace(3, None));
        assert_eq!(
            args[2],
            InstructionArg::literal(tari_bor::to_value(&123u64).unwrap()).unwrap()
        );
    }

    #[test]
    fn decode_encode_json() {
        let arg = InstructionArg::workspace(1, Some(2));
        let json = serde_json::to_string(&arg).unwrap();
        let decoded: InstructionArg = serde_json::from_str(&json).unwrap();
        assert_eq!(arg, decoded);

        let arg = InstructionArg::raw_literal_bytes(vec![1, 2, 3]);
        let json = serde_json::to_string(&arg).unwrap();
        let decoded: InstructionArg = serde_json::from_str(&json).unwrap();
        match decoded {
            InstructionArg::Literal(bytes) => assert_eq!(bytes.as_ref(), vec![1, 2, 3].as_slice()),
            _ => panic!("Expected literal"),
        }
    }

    #[test]
    fn decode_encode_binary() {
        let arg = InstructionArg::workspace(1, Some(2));
        let bytes = encode(&arg).unwrap();
        let decoded: InstructionArg = tari_bor::decode_exact(&bytes).unwrap();
        assert_eq!(arg, decoded);

        let arg = InstructionArg::raw_literal_bytes(vec![1, 2, 3]);
        let bytes = encode(&arg).unwrap();
        let decoded: InstructionArg = tari_bor::decode_exact(&bytes).unwrap();
        match decoded {
            InstructionArg::Literal(bytes) => assert_eq!(bytes.as_ref(), vec![1, 2, 3].as_slice()),
            _ => panic!("Expected literal"),
        }
    }
}
