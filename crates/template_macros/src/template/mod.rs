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

mod ast;
mod definition;
mod dispatcher;
pub mod options;
mod template_def;

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Result, parse2};

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
/// derives and indices by hand.
pub fn generate_template(options: TemplateOptions, input: TokenStream) -> Result<TokenStream> {
    let mut ast = parse2::<TemplateAst>(input)?;

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
