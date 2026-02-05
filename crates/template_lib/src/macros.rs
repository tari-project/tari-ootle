//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Rust macros that can be used inside templates

/// Macro for emitting log messages from inside templates
#[macro_export]
macro_rules! log {
    ($lvl:expr, $fmt:expr) => {
        $crate::engine().emit_log($lvl, $crate::template_macro_deps::rust::format!($fmt))
    };
    ($lvl:expr, $fmt:expr, $($args:tt)*) => {
        $crate::engine().emit_log($lvl, $crate::template_macro_deps::rust::format!($fmt, $($args)*))
    };
}

/// Macro for emitting debug log messages from inside templates
#[macro_export]
macro_rules! debug {
    ($fmt:expr) => {
        $crate::log!($crate::args::LogLevel::Debug, $fmt)
    };
    ($fmt:expr, $($args:tt)*) => {
        $crate::log!($crate::args::LogLevel::Debug, $fmt, $($args)*)
    };
}

/// Macro for emitting log messages from inside templates
#[macro_export]
macro_rules! info {
    ($fmt:expr) => {
        $crate::log!($crate::args::LogLevel::Info, $fmt)
    };
    ($fmt:expr, $($args:tt)*) => {
        $crate::log!($crate::args::LogLevel::Info, $fmt, $($args)*)
    };
}

/// Macro for emitting warn log messages from inside templates
#[macro_export]
macro_rules! warn {
    ($fmt:expr) => {
        $crate::log!($crate::args::LogLevel::Warn, $fmt)
    };
    ($fmt:expr, $($args:tt)*) => {
        $crate::log!($crate::args::LogLevel::Warn, $fmt, $($args)*)
    };
}

/// Macro for emitting error log messages from inside templates
#[macro_export]
macro_rules! error {
    ($fmt:expr) => {
        $crate::log!($crate::args::LogLevel::Error, $fmt)
    };
    ($fmt:expr, $($args:tt)*) => {
        $crate::log!($crate::args::LogLevel::Error, $fmt, $($args)*)
    };
}
