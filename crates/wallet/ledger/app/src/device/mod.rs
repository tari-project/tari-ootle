//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#[cfg(all(any(target_os = "nanosplus", target_os = "nanox"), not(feature = "nano_nbgl")))]
mod bagl;
#[cfg(all(any(target_os = "nanosplus", target_os = "nanox"), not(feature = "nano_nbgl")))]
pub use bagl::*;

#[cfg(any(target_os = "stax", target_os = "flex", target_os = "apex_p", feature = "nano_nbgl"))]
mod nbgl;
#[cfg(any(target_os = "stax", target_os = "flex", target_os = "apex_p", feature = "nano_nbgl"))]
pub use nbgl::*;
