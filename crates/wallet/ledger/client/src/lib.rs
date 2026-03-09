//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod client;
mod error;
#[cfg(feature = "hid-transport")]
mod hid;

pub use client::*;
pub use error::*;
#[cfg(feature = "hid-transport")]
pub use hid::*;

mod decode;
#[cfg(feature = "speculos-transport")]
pub mod speculos_transport;
