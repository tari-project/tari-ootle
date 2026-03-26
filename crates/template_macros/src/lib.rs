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

//! This crate provides the `template` procedural macro which generates the necessary boilerplate code to make a Rust
//! module work as a Tari Ootle template. The `template` macro generates the template definition, ABI functions and
//! dispatcher code from annotated template code.

mod template;

use proc_macro::TokenStream;

/// Generates Tari template definition and dispatcher code from annotated template code.
#[proc_macro_attribute]
pub fn template(_attr: TokenStream, item: TokenStream) -> TokenStream {
    template::generate_template(proc_macro2::TokenStream::from(item))
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}

/// Returns the template code without the wasm ABI code. This allows the code to compile for non-WASM targets and allows
/// "intellisense" to work in IDEs. Struct items within the module get serde derives injected so that
/// `Component::new` (which requires `T: serde::Serialize`) compiles on non-wasm targets.
#[proc_macro_attribute]
pub fn template_non_wasm(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut module: syn::ItemMod = match syn::parse(item.clone()) {
        Ok(m) => m,
        // If it doesn't parse as a module, return verbatim
        Err(_) => return item,
    };

    if let Some((brace, items)) = &mut module.content {
        let new_items = items
            .drain(..)
            .map(|item| match item {
                syn::Item::Struct(mut s) => {
                    let derive: syn::Attribute = syn::parse_quote! {
                        #[derive(::tari_template_lib::serde::Serialize, ::tari_template_lib::serde::Deserialize)]
                    };
                    s.attrs.push(derive);
                    syn::Item::Struct(s)
                },
                other => other,
            })
            .collect();
        module.content = Some((*brace, new_items));
    }

    quote::quote!(#module).into()
}
