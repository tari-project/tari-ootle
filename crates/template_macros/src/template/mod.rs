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

pub(crate) mod ast;
mod definition;
mod dispatcher;
pub mod options;
mod template_def;

use proc_macro2::TokenStream;
use quote::quote;
use syn::{ItemMod, Result, parse2};

use self::{
    ast::TemplateAst,
    definition::generate_definition,
    dispatcher::generate_dispatcher,
    options::TemplateOptions,
    template_def::generate_template_def,
};

/// Expand a `#[template(...)]` annotated module into the runtime template scaffolding
/// (definition, dispatcher, ABI).
///
/// `options` controls optional knobs parsed from the macro attribute. The default behaviour
/// (no flags) is to inject `#[derive(minicbor::Encode, Decode, CborLen)]` and positional
/// `#[n(N)]` tags onto every template struct/enum. Setting
/// [`TemplateOptions::skip_cbor_derives`] suppresses both, leaving the author to write the
/// derives and indices by hand. Setting [`TemplateOptions::stateless`] declares a
/// component-less template whose public API is a set of free `pub fn` items.
pub fn generate_template(options: TemplateOptions, input: TokenStream) -> Result<TokenStream> {
    // Parse to a module first so that AST construction can take `options` into account: the
    // `stateless` flag changes how the module body is interpreted (free functions vs. a component
    // struct/impl), which the `syn::Parse for TemplateAst` impl has no access to on its own.
    let module = parse2::<ItemMod>(input)?;
    let mut ast = TemplateAst::from_item_mod(module, options)?;

    if !options.skip_cbor_derives {
        ast::inject_cbor_derives(&mut ast.module_content)?;
    }

    let definition = generate_definition(&ast);
    let template_def = generate_template_def(&ast)?;
    let dispatcher = generate_dispatcher(&ast)?;

    let output = quote! {
        #definition

        #dispatcher

        #template_def
    };

    // eprintln!("output = {}", output);

    Ok(output)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use proc_macro2::TokenStream;

    use super::{generate_template, options::TemplateOptions};

    fn expand(src: &str, options: TemplateOptions) -> String {
        generate_template(options, TokenStream::from_str(src).unwrap())
            .unwrap()
            .to_string()
    }

    #[test]
    fn stateless_dispatches_to_free_functions() {
        let out = expand(
            "mod math { pub fn add(a: u32, b: u32) -> u32 { a + b } }",
            TemplateOptions {
                stateless: true,
                ..Default::default()
            },
        );

        assert!(out.contains("pub mod math_template"), "{out}");
        assert!(out.contains("math_main"), "{out}");
        // Free-function call path (`math_template :: add`), with no component struct qualifier.
        assert!(out.contains("math_template :: add"), "{out}");
        assert!(!out.contains("math_template :: math :: add"), "{out}");
    }

    #[test]
    fn stateless_composes_with_skip_cbor_derives() {
        let src = "mod math { pub struct Point { x: u32, y: u32 } pub fn sum(p: Point) -> u32 { p.x + p.y } }";

        // With derives injected (default): the data struct gets the minicbor derives.
        let with_derives = expand(src, TemplateOptions {
            stateless: true,
            ..Default::default()
        });
        assert!(with_derives.contains("minicbor :: Encode"), "{with_derives}");

        // skip_cbor_derives suppresses the injection even in stateless mode.
        let skipped = expand(src, TemplateOptions {
            stateless: true,
            skip_cbor_derives: true,
        });
        assert!(!skipped.contains("minicbor :: Encode"), "{skipped}");
    }
}
