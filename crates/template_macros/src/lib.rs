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

//! Procedural macros that drive the Tari Ootle template programming model.
//!
//! Most templates only ever interact with the [`template`] attribute, which expands an
//! annotated `mod` block into the runtime scaffolding the engine expects (template definition,
//! dispatcher, ABI exports, etc.). See [`tari_template_lib`] for the high-level
//! programming guide and an end-to-end example.
//!
//! ## Attribute flags
//!
//! `#[template]` accepts a comma-separated list of flags inside its parentheses. Currently
//! understood:
//!
//! - `skip_cbor_derives` — suppress the default `#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]`
//!   injection on template structs/enums *and* the per-field/variant `#[n(N)]` index assignment. The author is then
//!   responsible for providing their own derives and indices.
//!
//! Field-level overrides are always honoured, regardless of the flag: if a field already
//! carries `#[n(N)]`, `#[b(N)]`, or `#[cbor(n(N))]`, the macro will not overwrite it.
//!
//! ```ignore
//! use tari_template_lib::prelude::*;
//!
//! // Use the macro defaults — every field gets the next sequential #[n(N)].
//! #[template]
//! mod counter {
//!     pub struct Counter { value: u64 }
//!     impl Counter { pub fn new() -> Self { Self { value: 0 } } }
//! }
//!
//! // Override the wire format for a single field, leaving the rest auto-tagged.
//! #[template]
//! mod legacy {
//!     pub struct LegacyPair {
//!         // Pinned to #[n(7)] to preserve an existing on-disk format.
//!         #[n(7)] head: u32,
//!         tail: u32, // gets #[n(0)] from the macro (numbering restarts within each struct).
//!     }
//!     impl LegacyPair { pub fn new() -> Self { Self { head: 0, tail: 0 } } }
//! }
//!
//! // Take full control of derives and numbering.
//! #[template(skip_cbor_derives)]
//! mod hand_rolled {
//!     #[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
//!     pub struct HandRolled {
//!         #[n(0)] alpha: u32,
//!         #[n(1)] beta: u32,
//!     }
//!     impl HandRolled { pub fn new() -> Self { Self { alpha: 0, beta: 0 } } }
//! }
//! ```

mod template;

use proc_macro::TokenStream;
use syn::parse_macro_input;

use crate::template::options::TemplateOptions;

/// Generates Tari template definition and dispatcher code from annotated template code.
///
/// See the [crate-level docs](crate) for the list of supported attribute flags.
#[proc_macro_attribute]
pub fn template(attr: TokenStream, item: TokenStream) -> TokenStream {
    let options = parse_macro_input!(attr as TemplateOptions);
    template::generate_template(options, proc_macro2::TokenStream::from(item))
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}

/// Returns the template code without the wasm ABI code. This allows the code to compile for
/// non-WASM targets and allows "intellisense" to work in IDEs. The macro mirrors the WASM
/// path's CBOR injection so types stay encodable on host builds (tests, examples) where
/// `Component::new` requires `T: minicbor::Encode<()>`.
///
/// Honours [`TemplateOptions::skip_cbor_derives`] identically to [`template`].
#[proc_macro_attribute]
pub fn template_non_wasm(attr: TokenStream, item: TokenStream) -> TokenStream {
    let options = parse_macro_input!(attr as TemplateOptions);

    let mut module: syn::ItemMod = match syn::parse(item.clone()) {
        Ok(m) => m,
        // If it doesn't parse as a module, return verbatim
        Err(_) => return item,
    };

    if let Some((_, items)) = &mut module.content {
        if !options.skip_cbor_derives {
            if let Err(err) = template::ast::inject_cbor_derives(items) {
                return err.to_compile_error().into();
            }
        }

        // Bring `minicbor` into scope so the injected derives — and the code those
        // derives emit (which references `minicbor::Encoder`, `minicbor::Decoder`, …) —
        // resolve. The WASM path gets this via `use template_macro_deps::*` inside the
        // generated wrapper module; here we inject the same alias directly.
        items.insert(0, syn::parse_quote! {
            use ::tari_template_lib::template_macro_deps::minicbor;
        });
    }

    quote::quote!(#[allow(dead_code)] #module).into()
}
