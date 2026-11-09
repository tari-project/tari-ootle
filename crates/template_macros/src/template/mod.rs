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

/// Expand a `#[template(...)]` annotated module for non-WASM host targets.
///
/// Validates the template AST (and strips internal marker attributes such as `#[migration]`)
/// identically to [`generate_template`], while omitting the WASM ABI exports and custom section
/// so that templates compile cleanly under host `cargo check`, `cargo test`, and IDE analysis.
pub fn generate_template_non_wasm(options: TemplateOptions, mut module: ItemMod) -> Result<TokenStream> {
    let mut ast = TemplateAst::from_item_mod(module.clone(), options)?;

    // Run template definition and dispatcher validation so host builds surface the same
    // compile-time errors (e.g. invalid migration signatures, reserved template names, or
    // function hash collisions) as WASM builds.
    generate_template_def(&ast)?;
    generate_dispatcher(&ast)?;

    if !options.skip_cbor_derives {
        ast::inject_cbor_derives(&mut ast.module_content)?;

        // Bring `minicbor` into scope so the injected derives — and the code those
        // derives emit (which references `minicbor::Encoder`, `minicbor::Decoder`, …) —
        // resolve. The WASM path gets this via `use template_macro_deps::*` inside the
        // generated wrapper module; here we inject the same alias directly. Skipped
        // alongside the derives under `skip_cbor_derives` so authors rolling their own
        // encoding can manage their own `minicbor` import without collision.
        ast.module_content.insert(0, syn::parse_quote! {
            use ::tari_template_lib::template_macro_deps::minicbor;
        });
    }

    if let Some((_, items)) = &mut module.content {
        *items = ast.module_content;
    }

    Ok(quote!(#[allow(dead_code)] #module))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use proc_macro2::TokenStream;
    use syn::{ItemMod, parse2};

    use super::{generate_template, generate_template_non_wasm, options::TemplateOptions};

    fn expand(src: &str, options: TemplateOptions) -> String {
        generate_template(options, TokenStream::from_str(src).unwrap())
            .unwrap()
            .to_string()
    }

    fn expand_non_wasm(src: &str, options: TemplateOptions) -> syn::Result<String> {
        let module = parse2::<ItemMod>(TokenStream::from_str(src).unwrap()).unwrap();
        generate_template_non_wasm(options, module).map(|ts| ts.to_string())
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

    #[test]
    fn respects_explicit_cbor_index_attributes() {
        let src = r#"
            mod foo {
                pub struct Foo {
                    #[cbor(n(7))]
                    a: u32,
                    #[cbor(encode_with = "my_fn")]
                    b: u32,
                }
                impl Foo {
                    pub fn new() -> Self { Self { a: 0, b: 0 } }
                }
            }
        "#;
        let out = expand(src, TemplateOptions::default());
        // Injected tags are appended after existing attributes.
        assert!(!out.contains("# [cbor (n (7))] # [n (0)]"), "{out}");
        assert!(out.contains("# [cbor (encode_with = \"my_fn\")] # [n (1)]"), "{out}");
    }

    #[test]
    fn supports_associated_consts_trait_impls_and_nested_or_singleton_tuples() {
        let src = r#"
            mod foo {
                pub struct Foo { x: u32 }
                impl Default for Foo {
                    fn default() -> Self { Self { x: 0 } }
                }
                impl Foo {
                    pub const MAX: u32 = 100;
                    pub fn singleton(x: u32) -> (u32,) { (x,) }
                    pub fn nested(&self) -> ((u32, u64), u32) { ((self.x, 1), 2) }
                    #[migration]
                    pub fn migrate(old: (u32, u64)) -> Self { Self { x: old.0 } }
                }
            }
        "#;
        let out = expand(src, TemplateOptions::default());
        assert!(out.contains("(rtn . 0 ,)") || out.contains("(rtn.0,)"), "{out}");
        assert!(
            out.contains("component_manager . get_state :: < (u32 , u64) >") || out.contains("get_state::<(u32, u64)>"),
            "{out}"
        );
    }

    #[test]
    fn non_wasm_strips_migration_attribute_and_injects_cbor_derives() {
        let out = expand_non_wasm(
            r#"
            mod upgrade {
                pub struct CounterV2 { value: u64 }
                impl CounterV2 {
                    #[migration]
                    pub fn migrate(old: CounterV1) -> Self {
                        Self { value: 0 }
                    }
                }
            }
            "#,
            TemplateOptions::default(),
        )
        .unwrap();

        assert!(!out.contains("migration"), "residual #[migration] attribute: {out}");
        assert!(out.contains("minicbor :: Encode"), "{out}");
        assert!(
            out.contains("use :: tari_template_lib :: template_macro_deps :: minicbor"),
            "{out}"
        );
    }

    #[test]
    fn non_wasm_validates_template_ast_and_migration_signature() {
        // Missing previous state argument on a #[migration] function must fail on host builds too.
        let err = expand_non_wasm(
            r#"
            mod upgrade {
                pub struct CounterV2 { value: u64 }
                impl CounterV2 {
                    #[migration]
                    pub fn migrate() -> Self {
                        Self { value: 0 }
                    }
                }
            }
            "#,
            TemplateOptions::default(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("migration 'migrate'"), "{err}");

        // Reserved template name `std` must also be rejected on host builds.
        let err = expand_non_wasm(
            "mod std { pub fn add(a: u32, b: u32) -> u32 { a + b } }",
            TemplateOptions {
                stateless: true,
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("not permitted"), "{err}");
    }
}
