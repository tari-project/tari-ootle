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

use std::fmt::{Debug, Formatter};

use quote::ToTokens;
use syn::{
    Error,
    Field,
    Fields,
    FnArg,
    Ident,
    ImplItem,
    ImplItemFn,
    Item,
    ItemMod,
    ItemUse,
    Result,
    ReturnType,
    Type,
    TypePath,
    TypeTuple,
    UseTree,
    Variant,
    Visibility,
    parse::{Parse, ParseStream},
    parse_quote,
    punctuated::Punctuated,
    spanned::Spanned,
    token::Comma,
};

#[allow(dead_code)]
pub struct TemplateAst {
    pub template_name: Ident,
    pub module_content: Vec<Item>,
    pub functions: Vec<FunctionAst>,
    pub uses: Vec<ItemUse>,
}

impl Parse for TemplateAst {
    #[allow(clippy::too_many_lines)]
    fn parse(input: ParseStream) -> Result<Self> {
        // parse the "mod" block
        let mut module: ItemMod = input.parse()?;

        // get the contents of the "mod" block
        let items = match module.content {
            Some((_, ref mut items)) => items,
            None => return Err(Error::new(module.ident.span(), "empty module")),
        };

        let mut functions = Vec::with_capacity(5);

        // add derive macros to all structs
        let mut template_name = None;
        let mut has_impl = false;
        let mut uses = Vec::new();

        for item in items {
            match item {
                Item::Struct(item) => {
                    if !matches!(item.vis, Visibility::Public(_)) {
                        return Err(Error::new(item.ident.span(), "template structs must be public"));
                    }
                    item.attrs
                        .push(parse_quote!(#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]));
                    inject_field_tags(&mut item.fields)?;
                    // Use the first struct name as the template name
                    if template_name.is_none() {
                        template_name = Some(item.ident.clone());
                    }
                },
                Item::Enum(item) => {
                    if !matches!(item.vis, Visibility::Public(_)) {
                        return Err(Error::new(item.ident.span(), "template structs must be public"));
                    }
                    item.attrs
                        .push(parse_quote!(#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]));
                    inject_variant_tags(&mut item.variants)?;
                    if template_name.is_none() {
                        template_name = Some(item.ident.clone());
                    }
                },
                Item::Impl(impl_item) => {
                    if let Type::Path(path) = &*impl_item.self_ty {
                        let template_name_ref = template_name.as_ref().expect("struct not defined before impl");
                        if path.path.is_ident(template_name_ref) {
                            for impl_item_mut in &mut impl_item.items {
                                if let Some(func) = Self::get_function_from_item(impl_item_mut) {
                                    functions.push(func);
                                    if let ImplItem::Fn(fn_item) = impl_item_mut {
                                        let migration_pos =
                                            fn_item.attrs.iter().position(|attr| attr.path().is_ident("migration"));

                                        if let Some(migration_index) = migration_pos {
                                            match &fn_item.sig.output {
                                                ReturnType::Default => {
                                                    return Err(Error::new(
                                                        fn_item.sig.ident.span(),
                                                        "migration functions must return the new template struct. \
                                                         Found: unit",
                                                    ));
                                                },
                                                ReturnType::Type(_, ty) => match &**ty {
                                                    Type::Path(pth) => {
                                                        if !pth.path.is_ident(template_name_ref) &&
                                                            !pth.path.is_ident("Self")
                                                        {
                                                            return Err(Error::new(
                                                                ty.span(),
                                                                format!(
                                                                    "migration functions must return the new template \
                                                                     struct. Found: {}",
                                                                    pth.path.segments.last().unwrap().ident
                                                                ),
                                                            ));
                                                        }
                                                    },

                                                    ty => {
                                                        return Err(Error::new(
                                                            ty.span(),
                                                            format!(
                                                                "migration functions must return the new template \
                                                                 struct. Found: {:?}",
                                                                ty
                                                            ),
                                                        ));
                                                    },
                                                },
                                            }
                                            // Remove migration attribute from functions/methods
                                            fn_item.attrs.remove(migration_index);
                                        }
                                    }
                                }
                            }
                            has_impl = true;
                        }
                    }
                },
                Item::Use(item) => {
                    // Exclude super imports
                    if let UseTree::Path(path) = &item.tree &&
                        path.ident == "super"
                    {
                        continue;
                    }
                    uses.push(item.clone());
                },
                _ => {},
            }
        }

        if template_name.is_none() {
            return Err(Error::new(module.ident.span(), "a template must define a struct"));
        }

        if !has_impl {
            return Err(Error::new(
                module.ident.span(),
                "a template must have associated functions and/or methods",
            ));
        }

        Ok(Self {
            template_name: template_name.unwrap(),
            functions,
            module_content: module
                .content
                .map(|(_, c)| c)
                .ok_or_else(|| Error::new(module.ident.span(), "Template module must contain content"))?,
            uses,
        })
    }
}

/// Detects a `#[n(N)]` or `#[cbor(n(N))]` attribute already present on a field/variant.
fn has_explicit_cbor_index<'a, I: IntoIterator<Item = &'a syn::Attribute>>(attrs: I) -> bool {
    for attr in attrs {
        if attr.path().is_ident("n") || attr.path().is_ident("b") {
            return true;
        }
        if attr.path().is_ident("cbor") {
            // Crude but effective: parse the tokens and look for `n(...)` or `b(...)`.
            let s = attr.to_token_stream().to_string();
            if s.contains("n (") || s.contains("b (") {
                return true;
            }
        }
    }
    false
}

/// Append a `#[n(idx)]` attribute to every field in declaration order (skipping
/// fields that already carry an explicit `#[n(..)]`/`#[b(..)]`/`#[cbor(n(..))]`).
fn inject_field_tags(fields: &mut Fields) -> Result<()> {
    let mut idx: u32 = 0;
    let mut visit = |field: &mut Field| -> Result<()> {
        if !has_explicit_cbor_index(field.attrs.iter()) {
            let lit = syn::LitInt::new(&idx.to_string(), field.span());
            field.attrs.push(parse_quote!(#[n(#lit)]));
        }
        idx = idx
            .checked_add(1)
            .ok_or_else(|| Error::new(field.span(), "too many fields for #[template] minicbor tag assignment"))?;
        Ok(())
    };
    match fields {
        Fields::Named(named) => {
            for f in &mut named.named {
                visit(f)?;
            }
        },
        Fields::Unnamed(unnamed) => {
            for f in &mut unnamed.unnamed {
                visit(f)?;
            }
        },
        Fields::Unit => {},
    }
    Ok(())
}

/// Append a `#[n(idx)]` attribute to every enum variant in declaration order,
/// and tag each variant's payload fields the same way.
fn inject_variant_tags(variants: &mut Punctuated<Variant, Comma>) -> Result<()> {
    let mut idx: u32 = 0;
    for variant in variants.iter_mut() {
        if !has_explicit_cbor_index(variant.attrs.iter()) {
            let lit = syn::LitInt::new(&idx.to_string(), variant.span());
            variant.attrs.push(parse_quote!(#[n(#lit)]));
        }
        idx = idx.checked_add(1).ok_or_else(|| {
            Error::new(
                variant.span(),
                "too many enum variants for #[template] minicbor tag assignment",
            )
        })?;
        inject_field_tags(&mut variant.fields)?;
    }
    Ok(())
}

impl TemplateAst {
    pub fn get_functions(&self) -> impl Iterator<Item = &FunctionAst> + '_ {
        self.functions.iter()
    }

    fn get_function_from_item(item: &ImplItem) -> Option<FunctionAst> {
        match item {
            ImplItem::Fn(m) => {
                if !Self::is_public_function(m) {
                    return None;
                }
                Some(FunctionAst {
                    name: m.sig.ident.to_string(),
                    input_types: Self::get_input_types(&m.sig.inputs),
                    output_type: Self::get_output_type_token(&m.sig.output),
                    is_migration: m.attrs.iter().any(|attr| attr.path().is_ident("migration")),
                })
            },
            _ => todo!("get_function_from_item does not support anything other than functions/methods"),
        }
    }

    fn get_input_types(inputs: &Punctuated<FnArg, Comma>) -> Vec<TypeAst> {
        inputs
            .iter()
            .map(|arg| match arg {
                syn::FnArg::Receiver(r) => {
                    if r.reference.is_none() {
                        panic!("Consuming methods are not supported")
                    }

                    let mutability = r.mutability.is_some();
                    TypeAst::Receiver { mutability }
                },
                syn::FnArg::Typed(t) => Self::get_type_ast(Some(&t.pat), &t.ty),
            })
            .collect()
    }

    fn get_output_type_token(ast_type: &ReturnType) -> Option<TypeAst> {
        match ast_type {
            ReturnType::Default => None, // the function does not return anything
            ReturnType::Type(_, t) => Some(Self::get_type_ast(None, t)),
        }
    }

    fn get_type_ast(pat: Option<&syn::Pat>, syn_type: &syn::Type) -> TypeAst {
        match syn_type {
            syn::Type::Path(type_path) => {
                // TODO: handle "Self"
                // TODO: detect more complex types
                TypeAst::Typed {
                    name: pat.map(Self::get_pat_name),
                    type_path: type_path.clone(),
                }
            },
            syn::Type::Tuple(type_tuple) => TypeAst::Tuple {
                name: pat.map(Self::get_pat_name),
                type_tuple: type_tuple.clone(),
            },
            _ => todo!(
                "get_type_ast only supports paths and tuples. Encountered:{:?}",
                syn_type
            ),
        }
    }

    fn get_pat_name(pat: &syn::Pat) -> String {
        match pat {
            syn::Pat::Ident(ident) => ident.ident.to_string(),
            // There may be other patterns we are interested in, the following code
            // will print out the details, and the resulting code will not compile
            // but it will allow us to see the patterns we need.
            _ => format!("{:?}", pat),
        }
    }

    fn is_public_function(item: &ImplItemFn) -> bool {
        matches!(item.vis, syn::Visibility::Public(_))
    }
}

pub struct FunctionAst {
    pub name: String,
    pub input_types: Vec<TypeAst>,
    pub output_type: Option<TypeAst>,
    pub is_migration: bool,
    // pub statements: Vec<Stmt>,
    // pub is_constructor: bool,
    // pub is_public: bool,
}

impl FunctionAst {
    /// Returns true if the any argument is a &mut Self receiver
    pub fn is_mut(&self) -> bool {
        self.input_types
            .iter()
            .any(|t| matches!(t, TypeAst::Receiver { mutability: true }))
    }
}

pub enum TypeAst {
    Receiver {
        mutability: bool,
    },
    Typed {
        name: Option<String>,
        type_path: TypePath,
    },
    Tuple {
        name: Option<String>,
        type_tuple: TypeTuple,
    },
}

impl Debug for TypeAst {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            TypeAst::Receiver { mutability } => write!(f, "Receiver {{ mutability: {} }}", mutability),
            TypeAst::Typed { name, type_path } => write!(f, "Typed {{ name: {:?}, type_path: {:?} }}", name, type_path),
            TypeAst::Tuple { name, type_tuple } => {
                write!(f, "Tuple {{ name: {:?}, type_tuple: {:?} }}", name, type_tuple)
            },
        }
    }
}
