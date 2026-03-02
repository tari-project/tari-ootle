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

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{AngleBracketedGenericArguments, GenericArgument, PathArguments, PathSegment, Result, Type, TypeTuple};
use tari_template_abi::{
    ABI_TEMPLATE_DEF_GLOBAL_NAME,
    ArgDef,
    FunctionDef,
    TemplateDef,
    TemplateDefV1,
    Type as ArgType,
    version,
};

use crate::template::ast::{TemplateAst, TypeAst};

pub fn generate_abi(ast: &TemplateAst) -> Result<TokenStream> {
    let template_name_as_str = ast.template_name.to_string();

    let template_def = TemplateDef::V1(TemplateDefV1 {
        template_name: template_name_as_str.clone(),
        abi_version: version::LATEST_TEMPLATE_VERSION,
        functions: ast
            .get_functions()
            .map(|func| {
                let is_mut = func.is_mut();
                Ok::<_, syn::Error>(FunctionDef {
                    name: func.name.clone(),
                    arguments: func
                        .input_types
                        .iter()
                        .map(|ty| convert_to_arg_def(&template_name_as_str, ty))
                        .collect::<Result<_>>()?,
                    output: func
                        .output_type
                        .as_ref()
                        .map(|ty| convert_to_arg_type(&template_name_as_str, ty))
                        .unwrap_or(ArgType::Unit),
                    is_mut,
                    is_migration: func.is_migration,
                })
            })
            .collect::<Result<_>>()?,
    });

    let template_def_data = template_def.encode_for_wasm_embedding().map_err(|e| {
        syn::Error::new_spanned(
            &ast.template_name,
            format!("Failed to encode template definition: {}", e),
        )
    })?;
    let len = template_def_data.len();
    let template_def_name = format_ident!("{ABI_TEMPLATE_DEF_GLOBAL_NAME}");

    let output = quote! {
        #[unsafe(no_mangle)]
        pub static #template_def_name: [u8;#len] = [#(#template_def_data),*];
    };

    Ok(output)
}

fn convert_to_arg_type(template_name: &str, ty: &TypeAst) -> ArgType {
    match ty {
        TypeAst::Receiver { mutability: true } => ArgType::Other {
            name: "&mut self".to_string(),
        },
        TypeAst::Receiver { mutability: false } => ArgType::Other {
            name: "&self".to_string(),
        },
        TypeAst::Typed { type_path, .. } => path_segment_to_arg_type(template_name, type_path.path.segments.first()),
        TypeAst::Tuple { type_tuple, .. } => tuple_to_arg_type(template_name, type_tuple),
    }
}

fn convert_to_arg_def(template_name: &str, rust_type: &TypeAst) -> Result<ArgDef> {
    match rust_type {
        // on "&self" we want to pass the component id
        TypeAst::Receiver { mutability: false } => Ok(ArgDef {
            name: "self".to_string(),
            arg_type: ArgType::Other {
                name: "&self".to_string(),
            },
        }),
        TypeAst::Receiver { mutability: true } => Ok(ArgDef {
            name: "self".to_string(),
            arg_type: ArgType::Other {
                name: "&mut self".to_string(),
            },
        }),
        // basic type
        TypeAst::Typed {
            name: arg_name,
            type_path: path,
        } => {
            let Some(arg_name) = arg_name else {
                return Err(syn::Error::new_spanned(
                    path,
                    "convert_to_arg_def: Unnamed type is not valid in this context",
                ));
            };

            let arg_type = path_segment_to_arg_type(template_name, path.path.segments.first());

            Ok(ArgDef {
                name: arg_name.to_string(),
                arg_type,
            })
        },
        TypeAst::Tuple {
            name: arg_name,
            type_tuple,
        } => {
            let Some(arg_name) = arg_name else {
                return Err(syn::Error::new_spanned(
                    type_tuple,
                    "convert_to_arg_def: Unnamed type is not valid in this context",
                ));
            };
            let arg_type = tuple_to_arg_type(template_name, type_tuple);
            Ok(ArgDef {
                name: arg_name.to_string(),
                arg_type,
            })
        },
    }
}

fn path_segment_to_arg_type(template_name: &str, segment: Option<&PathSegment>) -> ArgType {
    match segment.map(|s| s.ident.to_string()) {
        None => ArgType::Unit,
        Some(ty) => match ty.as_str() {
            "" => ArgType::Unit,
            "bool" => ArgType::Bool,
            "i8" => ArgType::I8,
            "i16" => ArgType::I16,
            "i32" => ArgType::I32,
            "i64" => ArgType::I64,
            "i128" => ArgType::I128,
            "u8" => ArgType::U8,
            "u16" => ArgType::U16,
            "u32" => ArgType::U32,
            "u64" => ArgType::U64,
            "u128" => ArgType::U128,
            "String" => ArgType::String,
            "Vec" => {
                let inner = extract_single_generic_arg(template_name, segment.expect("segment is some"));
                ArgType::Vec(Box::new(inner))
            },
            "Option" => {
                let inner = extract_single_generic_arg(template_name, segment.expect("segment is some"));
                ArgType::Option(Box::new(inner))
            },
            "Self" => ArgType::Other {
                name: format!("Component<{}>", template_name),
            },
            type_name => {
                let seg = segment.expect("segment is some");
                let name = match &seg.arguments {
                    PathArguments::AngleBracketed(AngleBracketedGenericArguments { args, .. }) => {
                        let generic_args = args
                            .iter()
                            .map(|arg| match arg {
                                GenericArgument::Type(Type::Path(path)) => {
                                    let inner = path_segment_to_arg_type(template_name, path.path.segments.first());
                                    format!("{inner}")
                                },
                                GenericArgument::Type(Type::Tuple(tuple)) => {
                                    format!("{}", tuple_to_arg_type(template_name, tuple))
                                },
                                a => format!("{:?}", a),
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("{type_name}<{generic_args}>")
                    },
                    _ => type_name.to_string(),
                };
                ArgType::Other { name }
            },
        },
    }
}

fn extract_single_generic_arg(template_name: &str, segment: &PathSegment) -> ArgType {
    match &segment.arguments {
        PathArguments::AngleBracketed(AngleBracketedGenericArguments { args, .. }) => match &args[0] {
            GenericArgument::Type(Type::Path(path)) => {
                path_segment_to_arg_type(template_name, path.path.segments.first())
            },
            GenericArgument::Type(Type::Tuple(tuple)) => tuple_to_arg_type(template_name, tuple),
            a => panic!("Invalid generic argument {:?}", a),
        },
        _ => panic!("{} must specify a type argument: {:?}", segment.ident, segment),
    }
}

fn tuple_to_arg_type(template_name: &str, tuple: &TypeTuple) -> ArgType {
    let subtypes = tuple
        .elems
        .iter()
        .map(|t| {
            match t {
                Type::Path(path) => path_segment_to_arg_type(template_name, path.path.segments.first()),
                Type::Tuple(subtuple) => tuple_to_arg_type(template_name, subtuple),
                // TODO: These should be errors
                a => panic!("Invalid Tuple subtype argument {:?}", a),
            }
        })
        .collect::<Vec<_>>();

    ArgType::Tuple(subtypes)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use indoc::indoc;
    use proc_macro2::TokenStream;
    use syn::parse2;
    use tari_template_abi::{TemplateDef, Type as ArgType};

    use super::generate_abi;
    use crate::template::ast::TemplateAst;

    fn parse_template_def(input: &str) -> TemplateDef {
        let tokens = TokenStream::from_str(input).unwrap();
        let ast = parse2::<TemplateAst>(tokens).unwrap();
        let output = generate_abi(&ast).unwrap();

        // The generated code is: pub static TEMPLATE_DEF: [u8; N] = [b0, b1, ...] ;
        // Extract the byte array after "= ["
        let output_str = output.to_string();
        let eq_bracket = output_str.find("= [").unwrap();
        let data_start = eq_bracket + 2;
        let data_end = output_str[data_start..].find(']').unwrap() + data_start;
        let byte_list = &output_str[data_start + 1..data_end];

        let bytes: Vec<u8> = byte_list
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .map(|s| {
                let s = s.trim();
                // Handle suffixed literals like "128u8"
                let s = s.strip_suffix("u8").unwrap_or(s);
                s.parse::<u8>()
                    .unwrap_or_else(|e| panic!("Failed to parse byte '{}': {}", s, e))
            })
            .collect();

        // Skip the 4-byte length prefix
        tari_bor::decode::<TemplateDef>(&bytes[4..]).unwrap()
    }

    fn get_functions(def: &TemplateDef) -> &[tari_template_abi::FunctionDef] {
        match def {
            TemplateDef::V1(v1) => &v1.functions,
        }
    }

    #[test]
    fn test_primitive_types() {
        let def = parse_template_def(indoc! {"
            mod test_template {
                pub struct TestTemplate {}
                impl TestTemplate {
                    pub fn create(a: u32, b: String, c: bool, d: i64) -> u8 { }
                }
            }
        "});

        let funcs = get_functions(&def);
        assert_eq!(funcs.len(), 1);
        let f = &funcs[0];
        assert_eq!(f.name, "create");
        assert_eq!(f.arguments.len(), 4);
        assert_eq!(f.arguments[0].name, "a");
        assert_eq!(f.arguments[0].arg_type, ArgType::U32);
        assert_eq!(f.arguments[1].name, "b");
        assert_eq!(f.arguments[1].arg_type, ArgType::String);
        assert_eq!(f.arguments[2].name, "c");
        assert_eq!(f.arguments[2].arg_type, ArgType::Bool);
        assert_eq!(f.arguments[3].name, "d");
        assert_eq!(f.arguments[3].arg_type, ArgType::I64);
        assert_eq!(f.output, ArgType::U8);
    }

    #[test]
    fn test_option_type() {
        let def = parse_template_def(indoc! {"
            mod test_template {
                pub struct TestTemplate {}
                impl TestTemplate {
                    pub fn create(a: Option<String>, b: Option<u64>) { }
                }
            }
        "});

        let funcs = get_functions(&def);
        let f = &funcs[0];
        assert_eq!(f.arguments.len(), 2);
        assert_eq!(f.arguments[0].name, "a");
        assert_eq!(f.arguments[0].arg_type, ArgType::Option(Box::new(ArgType::String)));
        assert_eq!(f.arguments[1].name, "b");
        assert_eq!(f.arguments[1].arg_type, ArgType::Option(Box::new(ArgType::U64)));
    }

    #[test]
    fn test_vec_type() {
        let def = parse_template_def(indoc! {"
            mod test_template {
                pub struct TestTemplate {}
                impl TestTemplate {
                    pub fn create(a: Vec<u8>, b: Vec<String>) { }
                }
            }
        "});

        let funcs = get_functions(&def);
        let f = &funcs[0];
        assert_eq!(f.arguments[0].arg_type, ArgType::Vec(Box::new(ArgType::U8)));
        assert_eq!(f.arguments[1].arg_type, ArgType::Vec(Box::new(ArgType::String)));
    }

    #[test]
    fn test_nested_generic_types() {
        let def = parse_template_def(indoc! {"
            mod test_template {
                pub struct TestTemplate {}
                impl TestTemplate {
                    pub fn create(a: Vec<Option<String>>, b: Option<Vec<u32>>) { }
                }
            }
        "});

        let funcs = get_functions(&def);
        let f = &funcs[0];
        assert_eq!(
            f.arguments[0].arg_type,
            ArgType::Vec(Box::new(ArgType::Option(Box::new(ArgType::String))))
        );
        assert_eq!(
            f.arguments[1].arg_type,
            ArgType::Option(Box::new(ArgType::Vec(Box::new(ArgType::U32))))
        );
    }

    #[test]
    fn test_tuple_type() {
        let def = parse_template_def(indoc! {"
            mod test_template {
                pub struct TestTemplate {}
                impl TestTemplate {
                    pub fn create(a: (u32, String)) { }
                }
            }
        "});

        let funcs = get_functions(&def);
        let f = &funcs[0];
        assert_eq!(
            f.arguments[0].arg_type,
            ArgType::Tuple(vec![ArgType::U32, ArgType::String])
        );
    }

    #[test]
    fn test_custom_types_with_generics() {
        let def = parse_template_def(indoc! {"
            mod test_template {
                pub struct TestTemplate {}
                impl TestTemplate {
                    pub fn create(a: HashMap<String, u64>, b: ResourceAddress) { }
                }
            }
        "});

        let funcs = get_functions(&def);
        let f = &funcs[0];
        assert_eq!(f.arguments[0].arg_type, ArgType::Other {
            name: "HashMap<String, U64>".to_string()
        });
        assert_eq!(f.arguments[1].arg_type, ArgType::Other {
            name: "ResourceAddress".to_string()
        });
    }

    #[test]
    fn test_self_receiver() {
        let def = parse_template_def(indoc! {"
            mod test_template {
                pub struct TestTemplate {}
                impl TestTemplate {
                    pub fn create() -> Self { }
                    pub fn read(&self) -> u32 { }
                    pub fn write(&mut self, value: u32) { }
                }
            }
        "});

        let funcs = get_functions(&def);
        assert_eq!(funcs.len(), 3);

        // Constructor returning Self
        assert_eq!(funcs[0].name, "create");
        assert!(!funcs[0].is_mut);
        assert_eq!(funcs[0].output, ArgType::Other {
            name: "Component<TestTemplate>".to_string()
        });

        // &self method
        assert_eq!(funcs[1].name, "read");
        assert!(!funcs[1].is_mut);
        assert_eq!(funcs[1].arguments[0].name, "self");

        // &mut self method
        assert_eq!(funcs[2].name, "write");
        assert!(funcs[2].is_mut);
        assert_eq!(funcs[2].arguments[0].name, "self");
        assert_eq!(funcs[2].arguments[1].name, "value");
        assert_eq!(funcs[2].arguments[1].arg_type, ArgType::U32);
    }

    #[test]
    fn test_option_return_type() {
        let def = parse_template_def(indoc! {"
            mod test_template {
                pub struct TestTemplate {}
                impl TestTemplate {
                    pub fn find(name: String) -> Option<u64> { }
                }
            }
        "});

        let funcs = get_functions(&def);
        assert_eq!(funcs[0].output, ArgType::Option(Box::new(ArgType::U64)));
    }

    #[test]
    fn test_no_return_type() {
        let def = parse_template_def(indoc! {"
            mod test_template {
                pub struct TestTemplate {}
                impl TestTemplate {
                    pub fn do_something(a: u32) { }
                }
            }
        "});

        let funcs = get_functions(&def);
        assert_eq!(funcs[0].output, ArgType::Unit);
    }
}
