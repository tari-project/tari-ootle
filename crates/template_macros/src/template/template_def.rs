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

use std::collections::HashSet;

use proc_macro2::TokenStream;
use quote::{ToTokens, format_ident, quote};
use syn::{
    AngleBracketedGenericArguments,
    GenericArgument,
    Item,
    PathArguments,
    PathSegment,
    Result,
    Type,
    TypePath,
    TypeTuple,
};
use tari_template_abi::{
    ArgDef,
    FunctionDef,
    FunctionKind,
    Receiver,
    TEMPLATE_DEF_CUSTOM_SECTION,
    TemplateDef,
    TemplateDefV2,
    Type as ArgType,
    WellKnownType,
    version,
};

use crate::template::ast::{FunctionAst, TemplateAst, TypeAst};

/// Maximum length of a template name in bytes.
const MAX_TEMPLATE_NAME_LENGTH: usize = 64;

const RESERVED_TEMPLATE_NAME: &str = "std";

/// The template's definition section, and the checks that each type it tags as well-known is that type.
#[derive(Debug)]
pub struct GeneratedTemplateDef {
    pub section: TokenStream,
    /// Items for the template module, where the types in the template's signatures resolve.
    pub well_known_checks: TokenStream,
}

pub fn generate_template_def(ast: &TemplateAst) -> Result<GeneratedTemplateDef> {
    let template_name_as_str = ast.template_name.to_string();

    if template_name_as_str.len() > MAX_TEMPLATE_NAME_LENGTH {
        return Err(syn::Error::new_spanned(
            &ast.template_name,
            format!(
                "Template name '{}' exceeds the maximum length of {} bytes (got {} bytes)",
                template_name_as_str,
                MAX_TEMPLATE_NAME_LENGTH,
                template_name_as_str.len()
            ),
        ));
    }

    // The engine prefixes a template's event topics with its name, and topics starting with `std.`
    // belong to the engine's standard events.
    if template_name_as_str == RESERVED_TEMPLATE_NAME {
        return Err(syn::Error::new_spanned(
            &ast.template_name,
            format!("Template name '{RESERVED_TEMPLATE_NAME}' is not permitted"),
        ));
    }

    let mut types = TypeConverter::new(&template_name_as_str, &ast.module_content);
    let functions = ast
        .get_functions()
        .map(|func| {
            let (kind, params) = function_kind(&mut types, func)?;
            Ok::<_, syn::Error>(FunctionDef {
                name: func.name.clone(),
                kind,
                arguments: params
                    .iter()
                    .map(|ty| types.convert_to_arg_def(ty))
                    .collect::<Result<_>>()?,
                output: func
                    .output_type
                    .as_ref()
                    .map(|ty| types.convert_to_arg_type(ty))
                    .unwrap_or(ArgType::Unit),
            })
        })
        .collect::<Result<_>>()?;
    let template_def = TemplateDef::V2(TemplateDefV2 {
        template_name: template_name_as_str.clone(),
        abi_version: version::LATEST_TEMPLATE_VERSION,
        functions,
    });

    let template_def_data = template_def.encode_for_wasm_embedding().map_err(|e| {
        syn::Error::new_spanned(
            &ast.template_name,
            format!("Failed to encode template definition: {}", e),
        )
    })?;
    let len = template_def_data.len();
    let section_name = TEMPLATE_DEF_CUSTOM_SECTION;

    // Embed the `[u32 LE length] || [bor bytes]` blob into a WASM custom
    // section. Engines extract the bytes directly out of the section without
    // any linear-memory access. `#[used]` keeps the linker from stripping it;
    // LLVM's wasm backend preserves a non-standard `link_section` name as a
    // WASM custom section in the output binary.
    let section = quote! {
        #[used]
        #[unsafe(link_section = #section_name)]
        static _TARI_TEMPLATE_DEF: [u8; #len] = [#(#template_def_data),*];
    };

    Ok(GeneratedTemplateDef {
        section,
        well_known_checks: types.well_known_checks(),
    })
}

/// The function's kind, and the parameters the caller supplies: those after a method's receiver or a migration's
/// previous state.
fn function_kind<'a>(types: &mut TypeConverter<'_>, func: &'a FunctionAst) -> Result<(FunctionKind, &'a [TypeAst])> {
    match (func.is_migration, func.input_types.split_first()) {
        (true, Some((TypeAst::Receiver { .. }, _)) | None) => Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            format!(
                "migration '{}' must take the previous template's component state as its first argument, and no self",
                func.name
            ),
        )),
        (true, Some((from, params))) => Ok((
            FunctionKind::Migration {
                from: types.convert_to_arg_type(from),
            },
            params,
        )),
        (false, Some((TypeAst::Receiver { mutability }, params))) => {
            let receiver = if *mutability { Receiver::RefMut } else { Receiver::Ref };
            Ok((FunctionKind::Method(receiver), params))
        },
        (false, _) => Ok((FunctionKind::Function, &func.input_types)),
    }
}

/// Converts the template's Rust types to definition types, and records each one it tags as well-known.
struct TypeConverter<'a> {
    template_name: &'a str,
    /// Types the template module defines, which keep their own names even when a well-known type shares them.
    local_types: HashSet<String>,
    well_known: Vec<(TypePath, WellKnownType)>,
}

impl<'a> TypeConverter<'a> {
    fn new(template_name: &'a str, module_content: &[Item]) -> Self {
        let local_types = module_content
            .iter()
            .filter_map(|item| match item {
                Item::Struct(item) => Some(&item.ident),
                Item::Enum(item) => Some(&item.ident),
                Item::Type(item) => Some(&item.ident),
                Item::Union(item) => Some(&item.ident),
                _ => None,
            })
            .map(ToString::to_string)
            .collect();
        Self {
            template_name,
            local_types,
            well_known: Vec::new(),
        }
    }

    /// One `assert_well_known` per tagged type path, so that a path naming some other type fails the build.
    fn well_known_checks(&self) -> TokenStream {
        let mut seen = HashSet::new();
        let checks = self
            .well_known
            .iter()
            .filter(|(path, kind)| seen.insert((path.to_token_stream().to_string(), *kind)))
            .map(|(path, kind)| {
                let kind = format_ident!("{}", kind.name());
                quote! {
                    const _: () = ::tari_template_lib::well_known::assert_well_known::<#path>(
                        ::tari_template_lib::well_known::WellKnownType::#kind,
                    );
                }
            });
        quote! { #(#checks)* }
    }

    fn convert_to_arg_type(&mut self, ty: &TypeAst) -> ArgType {
        match ty {
            TypeAst::Receiver { mutability: true } => ArgType::Other {
                name: "&mut self".to_string(),
            },
            TypeAst::Receiver { mutability: false } => ArgType::Other {
                name: "&self".to_string(),
            },
            TypeAst::Typed { type_path, .. } => self.path_to_arg_type(type_path),
            TypeAst::Tuple { type_tuple, .. } => self.tuple_to_arg_type(type_tuple),
        }
    }

    fn convert_to_arg_def(&mut self, rust_type: &TypeAst) -> Result<ArgDef> {
        match rust_type {
            TypeAst::Receiver { .. } => unreachable!("receivers are taken out of the arguments before conversion"),
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

                Ok(ArgDef {
                    name: arg_name.to_string(),
                    arg_type: self.path_to_arg_type(path),
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
                Ok(ArgDef {
                    name: arg_name.to_string(),
                    arg_type: self.tuple_to_arg_type(type_tuple),
                })
            },
        }
    }

    fn path_to_arg_type(&mut self, type_path: &TypePath) -> ArgType {
        let Some(segment) = type_path.path.segments.last() else {
            return ArgType::Unit;
        };
        let ty = segment.ident.to_string();
        match ty.as_str() {
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
            "Vec" => ArgType::Vec(Box::new(self.extract_single_generic_arg(segment))),
            "Option" => ArgType::Option(Box::new(self.extract_single_generic_arg(segment))),
            "Self" => ArgType::Other {
                name: self.template_name.to_string(),
            },
            type_name => match self.well_known_type(type_path, type_name) {
                Some(kind) => {
                    self.well_known.push((type_path.clone(), kind));
                    ArgType::WellKnown(kind)
                },
                None => ArgType::Other {
                    name: self.other_type_name(type_name, segment),
                },
            },
        }
    }

    /// The well-known type that `type_path` names, unless the template module defines a type of that name.
    fn well_known_type(&self, type_path: &TypePath, type_name: &str) -> Option<WellKnownType> {
        let plain =
            type_path.qself.is_none() && matches!(type_path.path.segments.last()?.arguments, PathArguments::None);
        if !plain || self.local_types.contains(type_name) {
            return None;
        }
        WellKnownType::from_name(type_name)
    }

    /// The name of an `Other` type, with its generic arguments, whose types become text in the name. Only tagged types
    /// are checked, so these record no check.
    fn other_type_name(&mut self, type_name: &str, segment: &PathSegment) -> String {
        let PathArguments::AngleBracketed(AngleBracketedGenericArguments { args, .. }) = &segment.arguments else {
            return type_name.to_string();
        };
        let checked = self.well_known.len();
        let generic_args = args
            .iter()
            .map(|arg| match arg {
                GenericArgument::Type(Type::Path(path)) => self.path_to_arg_type(path).to_string(),
                GenericArgument::Type(Type::Tuple(tuple)) => self.tuple_to_arg_type(tuple).to_string(),
                a => a.to_token_stream().to_string(),
            })
            .collect::<Vec<_>>()
            .join(", ");
        self.well_known.truncate(checked);
        format!("{type_name}<{generic_args}>")
    }

    fn extract_single_generic_arg(&mut self, segment: &PathSegment) -> ArgType {
        match &segment.arguments {
            PathArguments::AngleBracketed(AngleBracketedGenericArguments { args, .. }) => {
                if args.len() != 1 {
                    panic!(
                        "{} must have exactly one generic type argument, but has {}",
                        segment.ident,
                        args.len()
                    );
                }
                match &args[0] {
                    GenericArgument::Type(Type::Path(path)) => self.path_to_arg_type(path),
                    GenericArgument::Type(Type::Tuple(tuple)) => self.tuple_to_arg_type(tuple),
                    a => panic!("Invalid generic argument {:?}", a),
                }
            },
            _ => panic!("{} must specify a type argument: {:?}", segment.ident, segment),
        }
    }

    fn tuple_to_arg_type(&mut self, tuple: &TypeTuple) -> ArgType {
        if tuple.elems.is_empty() {
            return ArgType::Unit;
        }
        let subtypes = tuple
            .elems
            .iter()
            .map(|t| {
                match t {
                    Type::Path(path) => self.path_to_arg_type(path),
                    Type::Tuple(subtuple) => self.tuple_to_arg_type(subtuple),
                    // TODO: These should be errors
                    a => panic!("Invalid Tuple subtype argument {:?}", a),
                }
            })
            .collect::<Vec<_>>();

        ArgType::Tuple(subtypes)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use indoc::indoc;
    use proc_macro2::TokenStream;
    use syn::parse2;
    use tari_template_abi::{FunctionKind, Receiver, TemplateDef, Type as ArgType, WellKnownType};

    use super::generate_template_def;
    use crate::template::{ast::TemplateAst, options::TemplateOptions};

    fn parse_template_def(input: &str) -> TemplateDef {
        let tokens = TokenStream::from_str(input).unwrap();
        let ast = parse2::<TemplateAst>(tokens).unwrap();
        decode_template_def(&ast)
    }

    fn parse_stateless_template_def(input: &str) -> TemplateDef {
        let module = parse2::<syn::ItemMod>(TokenStream::from_str(input).unwrap()).unwrap();
        let options = TemplateOptions {
            stateless: true,
            ..Default::default()
        };
        let ast = TemplateAst::from_item_mod(module, options).unwrap();
        decode_template_def(&ast)
    }

    fn decode_template_def(ast: &TemplateAst) -> TemplateDef {
        let output = generate_template_def(ast).unwrap().section;

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
            TemplateDef::V2(def) => &def.functions,
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
        assert_eq!(
            f.arguments[1].arg_type,
            ArgType::WellKnown(WellKnownType::ResourceAddress)
        );
    }

    #[test]
    fn test_const_generic_arg() {
        let def = parse_template_def(indoc! {"
            mod test_template {
                pub struct TestTemplate {}
                impl TestTemplate {
                    pub fn create(name: MaxString<8>) { }
                }
            }
        "});

        let funcs = get_functions(&def);
        let f = &funcs[0];
        assert_eq!(f.arguments[0].arg_type, ArgType::Other {
            name: "MaxString<8>".to_string()
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
        assert_eq!(funcs[0].kind, FunctionKind::Function);
        assert_eq!(funcs[0].output, ArgType::Other {
            name: "TestTemplate".to_string()
        });

        // &self method
        assert_eq!(funcs[1].name, "read");
        assert_eq!(funcs[1].kind, FunctionKind::Method(Receiver::Ref));
        assert!(funcs[1].arguments.is_empty());

        // &mut self method
        assert_eq!(funcs[2].name, "write");
        assert_eq!(funcs[2].kind, FunctionKind::Method(Receiver::RefMut));
        assert_eq!(funcs[2].arguments.len(), 1);
        assert_eq!(funcs[2].arguments[0].name, "value");
        assert_eq!(funcs[2].arguments[0].arg_type, ArgType::U32);
    }

    #[test]
    fn test_migration_takes_its_previous_state_out_of_the_arguments() {
        let def = parse_template_def(indoc! {"
            mod test_template {
                pub struct TestTemplate {}
                impl TestTemplate {
                    #[migration]
                    pub fn migrate(previous: TestTemplateV1, seed: u32) -> Self { }
                }
            }
        "});

        let funcs = get_functions(&def);
        assert_eq!(funcs[0].kind, FunctionKind::Migration {
            from: ArgType::Other {
                name: "TestTemplateV1".to_string()
            }
        });
        assert_eq!(funcs[0].arguments.len(), 1);
        assert_eq!(funcs[0].arguments[0].name, "seed");
        assert_eq!(funcs[0].num_call_args(), 2);
    }

    #[test]
    fn rejects_a_migration_without_a_previous_state() {
        let ast = parse2::<TemplateAst>(
            TokenStream::from_str(indoc! {"
                mod test_template {
                    pub struct TestTemplate {}
                    impl TestTemplate {
                        #[migration]
                        pub fn migrate() -> Self { }
                    }
                }
            "})
            .unwrap(),
        )
        .unwrap();

        let err = generate_template_def(&ast).unwrap_err();
        assert!(err.to_string().contains("migration 'migrate'"), "{err}");
    }

    #[test]
    fn test_qualified_paths_name_their_last_segment() {
        let def = parse_template_def(indoc! {"
            mod test_template {
                pub struct TestTemplate {}
                impl TestTemplate {
                    pub fn f(
                        amount: tari_template_lib::types::Amount,
                        pair: (std::string::String, types::Bucket),
                    ) -> core::option::Option<u64> { }
                }
            }
        "});

        let funcs = get_functions(&def);
        assert_eq!(
            funcs[0].arguments[0].arg_type,
            ArgType::WellKnown(WellKnownType::Amount)
        );
        assert_eq!(
            funcs[0].arguments[1].arg_type,
            ArgType::Tuple(vec![ArgType::String, ArgType::WellKnown(WellKnownType::Bucket)])
        );
        assert_eq!(funcs[0].output, ArgType::Option(Box::new(ArgType::U64)));
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

    #[test]
    fn test_explicit_unit_return_type() {
        let def = parse_template_def(indoc! {"
            mod test_template {
                pub struct TestTemplate {}
                impl TestTemplate {
                    pub fn do_something(a: u32) -> () { }
                }
            }
        "});

        let funcs = get_functions(&def);
        assert_eq!(funcs[0].output, ArgType::Unit);
    }

    #[test]
    fn test_stateless_template_def() {
        let def = parse_stateless_template_def(indoc! {"
            mod math {
                pub fn add(a: u32, b: u32) -> u32 { a + b }
                pub fn mul(a: u32, b: u32) -> u32 { a * b }
            }
        "});

        // Template name is taken from the module identifier, not a struct.
        let TemplateDef::V2(v2) = &def;
        assert_eq!(v2.template_name, "math");

        let funcs = get_functions(&def);
        assert_eq!(funcs.len(), 2);

        assert_eq!(funcs[0].name, "add");
        assert_eq!(funcs[0].arguments.len(), 2);
        assert_eq!(funcs[0].arguments[0].name, "a");
        assert_eq!(funcs[0].arguments[0].arg_type, ArgType::U32);
        assert_eq!(funcs[0].arguments[1].name, "b");
        assert_eq!(funcs[0].arguments[1].arg_type, ArgType::U32);
        assert_eq!(funcs[0].output, ArgType::U32);
        // No component, so no function takes a receiver.
        assert_eq!(funcs[0].kind, FunctionKind::Function);

        assert_eq!(funcs[1].name, "mul");
        assert_eq!(funcs[1].kind, FunctionKind::Function);
    }

    #[test]
    fn rejects_the_reserved_template_name() {
        let ast = parse2::<TemplateAst>(
            TokenStream::from_str(indoc! {"
                mod test_template {
                    pub struct std {}
                    impl std {
                        pub fn create() -> Self { }
                    }
                }
            "})
            .unwrap(),
        )
        .unwrap();
        let err = generate_template_def(&ast).unwrap_err();
        assert!(err.to_string().contains("not permitted"), "unexpected error: {err}");

        let module = parse2::<syn::ItemMod>(
            TokenStream::from_str("mod std { pub fn add(a: u32, b: u32) -> u32 { a + b } }").unwrap(),
        )
        .unwrap();
        let ast = TemplateAst::from_item_mod(module, TemplateOptions {
            stateless: true,
            ..Default::default()
        })
        .unwrap();
        let err = generate_template_def(&ast).unwrap_err();
        assert!(err.to_string().contains("not permitted"), "unexpected error: {err}");
    }

    fn well_known_checks(input: &str) -> String {
        let ast = parse2::<TemplateAst>(TokenStream::from_str(input).unwrap()).unwrap();
        generate_template_def(&ast).unwrap().well_known_checks.to_string()
    }

    const TAGGED: &str = indoc! {"
        mod test_template {
            pub struct TestTemplate {}
            impl TestTemplate {
                pub fn f(
                    bucket: Bucket,
                    ids: Vec<NonFungibleId>,
                    proof: Option<types::Proof>,
                    template: TemplateAddress,
                ) -> (Amount, Bucket) { }
            }
        }
    "};

    #[test]
    fn well_known_types_are_tagged_inside_vecs_options_and_tuples() {
        let funcs = get_functions(&parse_template_def(TAGGED)).to_vec();

        let types = funcs[0]
            .arguments
            .iter()
            .map(|a| a.arg_type.clone())
            .collect::<Vec<_>>();
        assert_eq!(types, vec![
            ArgType::WellKnown(WellKnownType::Bucket),
            ArgType::Vec(Box::new(ArgType::WellKnown(WellKnownType::NonFungibleId))),
            ArgType::Option(Box::new(ArgType::WellKnown(WellKnownType::Proof))),
            ArgType::WellKnown(WellKnownType::TemplateAddress),
        ]);
        assert_eq!(
            funcs[0].output,
            ArgType::Tuple(vec![
                ArgType::WellKnown(WellKnownType::Amount),
                ArgType::WellKnown(WellKnownType::Bucket)
            ])
        );
    }

    #[test]
    fn each_tagged_path_is_checked_once_as_the_template_wrote_it() {
        let checks = well_known_checks(TAGGED);

        for (path, kind) in [
            ("Bucket", "Bucket"),
            ("NonFungibleId", "NonFungibleId"),
            ("types :: Proof", "Proof"),
            ("TemplateAddress", "TemplateAddress"),
            ("Amount", "Amount"),
        ] {
            let check = format!(
                "assert_well_known :: < {path} > (:: tari_template_lib :: well_known :: WellKnownType :: {kind} ,)"
            );
            assert_eq!(checks.matches(&check).count(), 1, "{check} in {checks}");
        }
        assert_eq!(checks.matches("assert_well_known").count(), 5, "{checks}");
    }

    #[test]
    fn a_type_the_template_defines_keeps_its_name() {
        let input = indoc! {"
            mod test_template {
                pub struct TestTemplate {}
                impl TestTemplate {
                    pub fn f(bucket: Bucket) { }
                }
                pub struct Bucket {}
            }
        "};

        let funcs = get_functions(&parse_template_def(input)).to_vec();

        assert_eq!(funcs[0].arguments[0].arg_type, ArgType::Other {
            name: "Bucket".to_string()
        });
        assert_eq!(well_known_checks(input), "");
    }

    #[test]
    fn a_well_known_type_in_a_generic_name_is_not_checked() {
        let input = indoc! {"
            mod test_template {
                pub struct TestTemplate {}
                impl TestTemplate {
                    pub fn f(balances: HashMap<ResourceAddress, Amount>) { }
                }
            }
        "};

        let funcs = get_functions(&parse_template_def(input)).to_vec();

        assert_eq!(funcs[0].arguments[0].arg_type, ArgType::Other {
            name: "HashMap<ResourceAddress, Amount>".to_string()
        });
        assert_eq!(well_known_checks(input), "");
    }
}
