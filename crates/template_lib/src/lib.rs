//  Copyright 2022. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

//! This crate provides ergonomic abstractions that allow WASM templates to interact with the Tari Ootle engine.
//! Most if not all Ootle templates written in rust should depend on this crate.
//!
//! In most cases, you will only require the `prelude` which can be included with:
//! ```
//! use tari_template_lib::prelude::*;
//! ```
//!
//! Typically, a template author will use structs exported from the [models] module, the
//! [ResourceBuilder](resource::ResourceBuilder) and the [ComponentBuilder](component::ComponentBuilder). This crate
//! re-exports low-level ABI functions in `tari_template_abi` and the `tari_template_macros` proc macro.
//!
//! ## Template Examples
//!
//! - <https://github.com/tari-project/wasm-template>
//! - <https://github.com/tari-project/wasm-examples>
//! - <https://github.com/tari-project/tari-ootle/tree/development/crates/engine/tests/templates>
//!
//! ## no_std
//!
//! This crate supports `no_std` environments. To use in `no_std`, disable the `std` feature and enable the `alloc`
//! feature.

// Support no_std environments
#![cfg_attr(not(feature = "std"), no_std)]

// This can be uncommented if you need to check for mistaken use of the std crate
// TODO: to always use this, we'd need to include the rust prelude where ever ts_rs is used.
// #![no_std]
// #[cfg(feature = "std")]
// extern crate std;
#[cfg(not(any(feature = "std", feature = "alloc")))]
compile_error!("Either feature `std` or `alloc` must be enabled for this crate.");
#[cfg(all(target_arch = "wasm32", feature = "std", feature = "alloc"))]
compile_error!("Feature `std` and `alloc` can't be enabled at the same time.");

#[macro_use]
pub mod args;
#[macro_use]
pub mod models;

pub mod component;
mod consensus;
pub use consensus::Consensus;

pub mod caller_context;

pub mod rand;
pub mod resource;

pub mod events;

pub mod template;

pub use tari_template_lib_types as types;

// ---------------------------------------- WASM target exports ------------------------------------------------

pub mod template_macro_deps;

mod engine;
pub use engine::engine;

pub mod panic_hook;
pub mod prelude;
#[cfg(all(feature = "macro", target_arch = "wasm32"))]
pub use prelude::template;
// Re-export for macro
pub use tari_bor::to_value;

#[macro_use]
#[cfg(target_arch = "wasm32")]
pub mod macros;
