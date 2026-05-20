//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};
use tari_template_abi::rust::{
    fmt,
    fmt::{Display, Formatter},
    prelude::*,
    str::FromStr,
};

/// All the possible log levels
#[derive(Debug, Clone, Copy, Eq, PartialEq, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub enum LogLevel {
    #[n(0)]
    Error,
    #[n(1)]
    Warn,
    #[n(2)]
    Info,
    #[n(3)]
    Debug,
}

impl Display for LogLevel {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            LogLevel::Error => write!(f, "ERROR"),
            LogLevel::Warn => write!(f, "WARN"),
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Debug => write!(f, "DEBUG"),
        }
    }
}

impl FromStr for LogLevel {
    type Err = LogLevelParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ERROR" => Ok(LogLevel::Error),
            "WARN" => Ok(LogLevel::Warn),
            "INFO" => Ok(LogLevel::Info),
            "DEBUG" => Ok(LogLevel::Debug),
            _ => Err(LogLevelParseError(s.to_string())),
        }
    }
}

/// Error when trying to parse a log level from an `String`
#[derive(Debug)]
pub struct LogLevelParseError(String);

impl Display for LogLevelParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Failed to parse log level '{}'", self.0)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for LogLevelParseError {}
