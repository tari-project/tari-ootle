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
//! no_std can be enabled using the `no_std` feature flag.

pub mod auth;

#[macro_use]
pub mod args;
#[macro_use]
pub mod models;

pub mod component;
mod consensus;
pub use consensus::Consensus;

pub mod caller_context;
mod context;
pub use context::{get_context, init_context, AbiContext};

pub mod rand;
pub mod resource;

pub mod events;

pub mod template;

pub use tari_template_lib_types as types;

// ---------------------------------------- WASM target exports ------------------------------------------------

pub mod template_dependencies;

mod engine;
pub use engine::engine;

pub mod panic_hook;
pub mod prelude;
#[cfg(all(feature = "macro", target_arch = "wasm32"))]
pub use prelude::template;
// Re-export for macro
pub use tari_bor::to_value;

pub mod constants;

#[macro_use]
mod newtype_serde_macros;
#[macro_use]
pub mod macros;
