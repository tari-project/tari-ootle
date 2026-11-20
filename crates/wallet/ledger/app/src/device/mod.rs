//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#[cfg(all(any(target_os = "nanosplus", target_os = "nanox"), not(feature = "nbgl")))]
mod bagl;
#[cfg(all(any(target_os = "nanosplus", target_os = "nanox"), not(feature = "nbgl")))]
pub use bagl::*;

#[cfg(any(target_os = "stax", target_os = "flex", target_os = "apex_p", feature = "nbgl"))]
mod nbgl;
#[cfg(any(target_os = "stax", target_os = "flex", target_os = "apex_p", feature = "nbgl"))]
pub use nbgl::*;
