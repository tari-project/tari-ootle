//  Copyright 2022. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that
// the  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the
// following  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED
// WARRANTIES,  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A
// PARTICULAR PURPOSE ARE  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY
// DIRECT, INDIRECT, INCIDENTAL,  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
// PROCUREMENT OF SUBSTITUTE GOODS OR  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
// CAUSED AND ON ANY THEORY OF LIABILITY,  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR
// OTHERWISE) ARISING IN ANY WAY OUT OF THE  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH
// DAMAGE.

//! Definitions and utilities related to instruction arguments

mod types;
pub use types::*;

mod arg;
pub use arg::*;

mod result;
pub use result::*;

/// Low-level macro used for counting characters in the encoding of arguments. Not intended for general usage
#[macro_export]
macro_rules! __expr_counter {
    () => (0usize);
    ( $x:expr $(,)? ) => (1usize);
    ( $x:expr, $($next:tt)* ) => (1usize + $crate::__expr_counter!($($next)*));
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

/// Low-level macro for building instruction arguments, used by both `arg!` and `args!` macros. Not intended for general
/// usage.
#[macro_export]
macro_rules! __args_inner {
    (@ { $this:ident } Workspace($e:expr), $($tail:tt)*) => {
        $crate::args::__push(&mut $this, $crate::call_arg!(Workspace($e)));
        $crate::__args_inner!(@ { $this } $($tail)*);
    };

    (@ { $this:ident } Workspace($e:expr) $(,)?) => {
        $crate::args::__push(&mut $this, $crate::call_arg!(Workspace($e)));
    };

    (@ { $this:ident } Literal($e:expr), $($tail:tt)*) => {
        $crate::args::__push(&mut $this, $crate::call_arg!(Literal($e)));
        $crate::__args_inner!(@ { $this } $($tail)*);
    };

    (@ { $this:ident } Literal($e:expr) $(,)?) => {
        $crate::args::__push(&mut $this, $crate::call_arg!(Literal($e)));
    };

    // Special handling to allow Amount(x) syntax
    (@ { $this:ident } Amount($e:expr) $(,)?) => {
        $crate::args::__push(&mut $this, $crate::call_arg!(Amount($e)));
    };

    (@ { $this:ident } $e:expr, $($tail:tt)*) => {
        $crate::args::__push(&mut $this, $crate::call_arg!(Literal($e)));
        $crate::__args_inner!(@ { $this } $($tail)*);
    };

    (@ { $this:ident } $e:expr $(,)*) => {
        $crate::args::__push(&mut $this, $crate::call_arg!(Literal($e)));
    };

    (@ { $this:ident } $(,)?) => { };
}

/// Low-level macro used for encoding the arguments of engine calls. Not intended for general usage
#[macro_export]
macro_rules! invoke_args {
    () => (Vec::new());

    ($($args:expr),+) => {{
        let mut args = Vec::<Vec<u8>>::with_capacity($crate::__expr_counter!($($args),+));
        $(
            $crate::args::__push(&mut args, tari_bor::encode(&$args).unwrap());
        )+
        args
    }}
}

/// Utility macro for building multiple instruction arguments
#[macro_export]
macro_rules! call_args {
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
}
