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

use std::collections::HashMap;

use proc_macro2::{Ident, Span, TokenStream};
use quote::{format_ident, quote};
use syn::{Block, Expr, ExprBlock, ExprField, Result, Stmt, TypePath, TypeTuple, parse_quote, token::Brace};
use tari_template_abi::{FunctionIdent, diagnostics::PanicDiagnostic, func_hasher::hash_function_name};

use crate::template::ast::{FunctionAst, TemplateAst, TypeAst};

pub fn generate_dispatcher(ast: &TemplateAst) -> Result<TokenStream> {
    let dispatcher_function_name = format_ident!("{}_main", ast.template_name);
    let function_idents = get_function_idents(ast)?;
    let function_blocks = get_function_blocks(ast);

    // Emitted inside `#template_mod_name` so every type path in the template's signatures resolves in the
    // same scope as the template's own items, while `const _` keeps the export out of the module's namespace.
    let output = quote! {
        const _: () = {
            #[unsafe(no_mangle)]
            pub unsafe extern "C" fn #dispatcher_function_name(
                call_info_ptr: *mut u8,
                _call_info_len: u32,
            ) -> *mut u8 {
                #[cfg(not(target_arch = "wasm32"))]
                compile_error!("Must compile template with --target wasm32-unknown-unknown");

                // Custom panic hook that invokes the on_panic host function
                ::tari_template_lib::template_macro_deps::register_panic_hook();

                if call_info_ptr.is_null() {
                    panic!("CALLINFO_NULL");
                }

                let owned = unsafe {
                    ::tari_template_lib::template_macro_deps::OwnedData::owned_from_ptr(call_info_ptr)
                };
                let mut call_info =
                    ::tari_template_lib::template_macro_deps::CallInfo::v1_packed_reader(owned.data());
                let call_header = call_info.decode_header();

                let result_ptr;
                match call_header.func {
                    #( #function_idents => #function_blocks ),*,
                    _ => panic!("UNKWN_FN {}", call_header.func),
                };

                result_ptr
            }
        };
    };

    Ok(output)
}

fn get_function_idents(ast: &TemplateAst) -> Result<impl Iterator<Item = FunctionIdent> + '_> {
    let mut collisions = HashMap::with_capacity(ast.functions.len());
    // Collect the idents to preserve order
    let mut idents = Vec::with_capacity(ast.functions.len());
    for f in ast.get_functions() {
        let ident = hash_function_name(&f.name);
        idents.push(ident);
        if let Some(other) = collisions.insert(ident, &f.name) {
            // NOTE: if a user has the maximum permitted functions (8192) the chance of collision is ~0.778% (<8 in
            // 1000) (birthday problem: p(k) = 1 - exp(-k(k-1)/2N) where k is the number of hashes).
            // Moreover, This check is not strictly necessary as the dispatcher will fail to compile due to duplicate
            // match arms. This is to provide a clearer error message to the user.
            return Err(syn::Error::new(
                Span::call_site(),
                format!(
                    "Function name hash collision detected between '{}' and '{}' (hash = {}). Please rename one of \
                     these functions to avoid the collision.",
                    other, f.name, ident
                ),
            ));
        }
    }
    Ok(idents.into_iter())
}

fn get_function_blocks(ast: &TemplateAst) -> impl Iterator<Item = Expr> + '_ {
    ast.get_functions()
        .map(|function| get_function_block(&ast.template_name, function, ast.stateless))
}

#[allow(clippy::too_many_lines)]
fn get_function_block(template_ident: &Ident, ast: &FunctionAst, stateless: bool) -> Expr {
    let template_mod_name = format_ident!("{}_template", template_ident);
    let mut args: Vec<Expr> = vec![];
    let mut stmts = vec![];
    // The engine renders these markers into their full text from the template definition, so the
    // published binary carries neither the function name nor the argument type. See
    // `tari_template_abi::diagnostics`.
    let error_failed_decode_component = PanicDiagnostic::ComponentDecode.panic_format_string();

    let mut is_mutable_call = false;

    // encode all arguments of the functions
    for (i, input_type) in ast.input_types.iter().enumerate() {
        let arg_ident = format_ident!("arg_{}", i);

        match input_type {
            // "self" argument
            TypeAst::Receiver { mutability } => {
                assert!(
                    !ast.is_migration,
                    "migration functions cannot have &self or &mut self arguments"
                );
                is_mutable_call = *mutability;
                if is_mutable_call {
                    args.push(parse_quote! { &mut state });
                } else {
                    args.push(parse_quote! { &state });
                }
                stmts.extend([
                    parse_quote! { let next_arg = call_info.next_arg_unchecked(); },
                    parse_quote! {
                        let component_address = ::tari_template_lib::template_macro_deps::decode_exact::<
                            ::tari_template_lib::types::ComponentAddress,
                        >(next_arg)
                        .unwrap_or_else(|e| panic!(#error_failed_decode_component, e));
                    },
                    parse_quote! {
                        let component_manager =
                            ::tari_template_lib::template_macro_deps::engine().component_manager(component_address);
                    },
                    parse_quote! {
                        let mut state = component_manager.get_state::<super::#template_mod_name::#template_ident>();
                    },
                ]);
            },
            // non-self argument
            TypeAst::Typed { type_path, .. } => {
                let error_failed_decode_arg = PanicDiagnostic::ArgDecode { index: i }.panic_format_string();
                if i == 0 && ast.is_migration {
                    stmts.extend([
                        parse_quote! { let next_arg = call_info.next_arg_unchecked(); },
                        parse_quote! {
                            let component_address = ::tari_template_lib::template_macro_deps::decode_exact::<
                                ::tari_template_lib::types::ComponentAddress,
                            >(next_arg)
                            .unwrap_or_else(|e| panic!(#error_failed_decode_component, e));
                        },
                        parse_quote! {
                            let component_manager =
                                ::tari_template_lib::template_macro_deps::engine().component_manager(component_address);
                        },
                        parse_quote! {
                            let old_state = component_manager.get_state::<#type_path>();
                        },
                    ]);
                    args.push(parse_quote! { old_state });
                } else {
                    args.push(parse_quote! { #arg_ident });
                    stmts.extend([
                        parse_quote! { let next_arg = call_info.next_arg_unchecked(); },
                        parse_quote! {
                            let #arg_ident = ::tari_template_lib::template_macro_deps::decode_exact::<#type_path>(next_arg)
                                .unwrap_or_else(|e| panic!(#error_failed_decode_arg, e));
                        },
                    ]);
                }
            },
            TypeAst::Tuple { type_tuple, .. } => {
                let error_failed_decode_arg = PanicDiagnostic::ArgDecode { index: i }.panic_format_string();
                if i == 0 && ast.is_migration {
                    stmts.extend([
                        parse_quote! { let next_arg = call_info.next_arg_unchecked(); },
                        parse_quote! {
                            let component_address = ::tari_template_lib::template_macro_deps::decode_exact::<
                                ::tari_template_lib::types::ComponentAddress,
                            >(next_arg)
                            .unwrap_or_else(|e| panic!(#error_failed_decode_component, e));
                        },
                        parse_quote! {
                            let component_manager =
                                ::tari_template_lib::template_macro_deps::engine().component_manager(component_address);
                        },
                        parse_quote! {
                            let old_state = component_manager.get_state::<#type_tuple>();
                        },
                    ]);
                    args.push(parse_quote! { old_state });
                } else {
                    args.push(parse_quote! { #arg_ident });
                    stmts.extend([
                        parse_quote! { let next_arg = call_info.next_arg_unchecked(); },
                        parse_quote! {
                            let #arg_ident = ::tari_template_lib::template_macro_deps::decode_exact::<#type_tuple>(next_arg)
                                .unwrap_or_else(|e| panic!(#error_failed_decode_arg, e));
                        },
                    ]);
                }
            },
        }
    }

    // call the user defined function in the template
    let function_ident = &ast.ident;
    stmts.push(if stateless {
        // Stateless templates expose free functions directly in the generated module, so there is
        // no component struct to qualify the call with.
        parse_quote! {
            let rtn = super::#template_mod_name::#function_ident(#(#args),*);
        }
    } else {
        parse_quote! {
            let rtn = super::#template_mod_name::#template_ident::#function_ident(#(#args),*);
        }
    });

    if ast.is_migration {
        stmts.extend([
            // Empty result
            parse_quote! { result_ptr = ::tari_template_lib::template_macro_deps::alloc_and_encode(&()); },
            // Set the state to the return value
            parse_quote! { component_manager.set_state(rtn); },
        ]);
    } else {
        // Handle "Self" (if present) in the return position by creating a new component
        stmts.extend(replace_self_in_output(ast));

        // encode the result value
        stmts.push(parse_quote! {
            result_ptr = ::tari_template_lib::template_macro_deps::alloc_and_encode(&rtn);
        });

        // after user function invocation, update the component state
        if is_mutable_call {
            stmts.push(parse_quote! { component_manager.set_state(state); });
        }
    }

    // construct the code block for the function
    Expr::Block(ExprBlock {
        attrs: vec![],
        label: None,
        block: Block {
            brace_token: Brace::default(),
            stmts,
        },
    })
}

fn replace_self_in_output(ast: &FunctionAst) -> Vec<Stmt> {
    if let Some(output_type) = &ast.output_type {
        match output_type {
            TypeAst::Typed { type_path, .. } => {
                if let Some(stmt) = replace_self_in_single_value(type_path) {
                    return vec![stmt];
                }
            },
            TypeAst::Tuple { type_tuple, .. } => {
                let stmt = replace_self_in_tuple(type_tuple);
                return vec![stmt];
            },
            _ => todo!("replace_self_in_output only supports typed and tuple"),
        }
    }

    vec![]
}

fn replace_self_in_single_value(type_path: &TypePath) -> Option<Stmt> {
    let type_ident = &type_path.path.segments.first()?.ident;

    if type_ident == "Self" {
        // When we return self we use default rules - which only permit the owner of the component to call methods
        return Some(parse_quote! {
            let rtn = ::tari_template_lib::template_macro_deps::engine().create_component(
                rtn,
                ::tari_template_lib::template_macro_deps::OwnerRule::default(),
                ::tari_template_lib::template_macro_deps::ComponentAccessRules::new(),
                None,
                ::tari_template_lib::template_macro_deps::UpdateRule::Locked,
                ::tari_template_lib::template_macro_deps::RulesUpdater::Locked,
            );
        });
    }

    None
}

fn replace_self_in_tuple(type_tuple: &TypeTuple) -> Stmt {
    let tuple_expr = replace_self_in_tuple_expr(parse_quote!(rtn), type_tuple);
    parse_quote! {
        let rtn = #tuple_expr;
    }
}

fn replace_self_in_tuple_expr(base_expr: Expr, type_tuple: &TypeTuple) -> Expr {
    if type_tuple.elems.is_empty() {
        return parse_quote! { () };
    }

    // build the expressions for each element in the tuple
    let elems: Vec<Expr> = type_tuple
        .elems
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let field_expr = build_tuple_field_expr(base_expr.clone(), i as u32);
            match t {
                syn::Type::Path(path) => {
                    let ident = path
                        .path
                        .segments
                        .first()
                        .expect("path segments is empty")
                        .ident
                        .clone();
                    if ident == "Self" {
                        // When we return self we use default rules - which only permit the owner of the component to
                        // call methods
                        parse_quote! {
                            ::tari_template_lib::template_macro_deps::engine().create_component(
                                #field_expr,
                                ::tari_template_lib::template_macro_deps::OwnerRule::default(),
                                ::tari_template_lib::template_macro_deps::ComponentAccessRules::new(),
                                None,
                                ::tari_template_lib::template_macro_deps::UpdateRule::Locked,
                                ::tari_template_lib::template_macro_deps::RulesUpdater::Locked,
                            )
                        }
                    } else {
                        field_expr
                    }
                },
                syn::Type::Tuple(subtuple) => replace_self_in_tuple_expr(field_expr, subtuple),
                _ => todo!("replace_self_in_tuple only supports paths and tuples"),
            }
        })
        .collect();

    if elems.len() == 1 {
        parse_quote! { (#(#elems,)*) }
    } else {
        parse_quote! { (#(#elems),*) }
    }
}

fn build_tuple_field_expr(base: Expr, i: u32) -> Expr {
    let mut field_expr: ExprField = parse_quote! {
        #base.0
    };

    match field_expr.member {
        syn::Member::Unnamed(ref mut unnamed) => {
            unnamed.index = i;
        },
        _ => todo!("build_tuple_field_expr only supports Unnamed"),
    }

    Expr::Field(field_expr)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use indoc::indoc;
    use proc_macro2::TokenStream;
    use quote::ToTokens;
    use syn::{Expr, ExprBlock, Local, LocalInit, Pat, PatIdent, Stmt, parse2};

    use super::{generate_dispatcher, get_function_block};
    use crate::template::ast::TemplateAst;

    /// The value the dispatcher arm for `name` encodes, re-parsed from the emitted tokens.
    fn encoded_return(ast: &TemplateAst, name: &str) -> Expr {
        let function = ast.get_functions().find(|f| f.name == name).unwrap();
        let block = get_function_block(&ast.template_name, function, ast.stateless);
        let block = parse2::<ExprBlock>(block.to_token_stream()).unwrap();
        block
            .block
            .stmts
            .iter()
            .rev()
            .find_map(|stmt| match stmt {
                Stmt::Local(Local {
                    pat: Pat::Ident(PatIdent { ident, .. }),
                    init: Some(LocalInit { expr, .. }),
                    ..
                }) if ident == "rtn" => Some((**expr).clone()),
                _ => None,
            })
            .unwrap()
    }

    fn tuple_elems(expr: &Expr) -> Vec<&Expr> {
        let Expr::Tuple(tuple) = expr else {
            panic!("expected a tuple, got {}", expr.to_token_stream());
        };
        tuple.elems.iter().collect()
    }

    /// The component state passed to `create_component`.
    fn created_component(expr: &Expr) -> String {
        let Expr::MethodCall(call) = expr else {
            panic!("expected create_component, got {}", expr.to_token_stream());
        };
        assert_eq!(call.method, "create_component");
        call.args.first().unwrap().to_token_stream().to_string()
    }

    #[test]
    fn supports_nested_and_unit_tuple_returns() {
        let ast = parse2::<TemplateAst>(
            TokenStream::from_str(indoc! {"
                mod tuple_test {
                    pub struct TupleComponent {}
                    impl TupleComponent {
                        pub fn unit_ret(&self) -> () { }
                        pub fn nested_ret(&self) -> (u32, (u64, String)) { (1, (2, String::new())) }
                        pub fn nested_self_ret() -> (u32, (Self, String)) { (1, (Self {}, String::new())) }
                        pub fn single_self_tuple() -> (Self,) { (Self {},) }
                    }
                }
            "})
            .unwrap(),
        )
        .unwrap();

        generate_dispatcher(&ast).unwrap();

        // `unit_ret` encodes the unit value.
        assert!(tuple_elems(&encoded_return(&ast, "unit_ret")).is_empty());

        // `nested_ret` encodes each field of the nested tuple unchanged.
        let nested = encoded_return(&ast, "nested_ret");
        let outer = tuple_elems(&nested);
        assert_eq!(outer.len(), 2);
        assert_eq!(outer[0].to_token_stream().to_string(), "rtn . 0");
        let inner = tuple_elems(outer[1]);
        assert_eq!(inner.len(), 2);
        assert_eq!(inner[0].to_token_stream().to_string(), "rtn . 1 . 0");
        assert_eq!(inner[1].to_token_stream().to_string(), "rtn . 1 . 1");

        // `nested_self_ret` creates a component from `rtn.1.0` and encodes `rtn.0` and `rtn.1.1` unchanged.
        let nested_self = encoded_return(&ast, "nested_self_ret");
        let outer = tuple_elems(&nested_self);
        assert_eq!(outer.len(), 2);
        assert_eq!(outer[0].to_token_stream().to_string(), "rtn . 0");
        let inner = tuple_elems(outer[1]);
        assert_eq!(inner.len(), 2);
        assert_eq!(created_component(inner[0]), "rtn . 1 . 0");
        assert_eq!(inner[1].to_token_stream().to_string(), "rtn . 1 . 1");

        // `single_self_tuple` encodes a 1-tuple, so the emitted tokens keep the trailing comma.
        let single = encoded_return(&ast, "single_self_tuple");
        let elems = tuple_elems(&single);
        assert_eq!(elems.len(), 1);
        assert_eq!(created_component(elems[0]), "rtn . 0");
    }
}
