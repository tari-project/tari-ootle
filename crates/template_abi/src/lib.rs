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

#![cfg_attr(not(feature = "std"), no_std)]

//! # Tari WASM module ABI (application binary interface)
//!
//! This library provides types and encoding that allow low-level communication between the Tari WASM runtime and the
//! WASM modules.

mod abi;
pub use abi::*;

mod call_info;
pub use call_info::*;

mod ops;
pub use ops::*;

pub mod rust;

mod template_def;
pub use template_def::*;

pub mod version;

#[cfg(feature = "func-hasher")]
pub mod func_hasher;

/// The name of the global export that defines the template definition.
///
/// NOTE: this is deprecated in favour  of the `tari_tdef' custom section and will be removed.
///
/// This is the legacy embedding path: the template's bor-encoded `TemplateDef`
/// (with a 4-byte LE length prefix) is laid out in the WASM module's linear
/// memory and an exported i32 global of this name holds the pointer to the
/// length prefix. Engines read it via instance-time memory access.
pub const ABI_TEMPLATE_DEF_GLOBAL_NAME: &str = "_ABI_TEMPLATE_DEF";

/// The name of the WASM custom section that holds the template definition.
///
/// New templates additionally embed the same `[u32 LE length] || [bor bytes]`
/// blob as a WASM custom section. This avoids any dependency on linear-memory
/// layout, which makes static extraction trivial (no global / data-segment
/// walk) and gives the JIT path a way to recover the ABI without instantiating
/// the module. The legacy global+rodata embedding is kept alongside this for
/// backward compatibility with engines that don't yet read custom sections.
pub const TEMPLATE_DEF_CUSTOM_SECTION: &str = "tari_tdef";

pub const WASM_PTR_SIZE: usize = 4; // 32-bit pointers in wasm
