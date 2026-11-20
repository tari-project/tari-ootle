//  Copyright 2020, The Tari Project
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
use quote::ToTokens;
use syn::{
    FnArg,
    GenericArgument,
    ItemTrait,
    Meta,
    PathArguments,
    ReturnType,
    Token,
    TraitItem,
    TraitItemFn,
    Type,
    fold,
    fold::Fold,
    punctuated::Punctuated,
};

use crate::{generator::RpcCodeGenerator, method_info::RpcMethodInfo, options::RpcTraitOptions};

pub fn expand_trait(node: ItemTrait, options: RpcTraitOptions) -> TokenStream {
    let mut collector = TraitInfoCollector::new();
    let trait_code = collector.fold_item_trait(node);
    let generator = RpcCodeGenerator::new(options, collector.expect_trait_ident(), collector.rpc_methods);
    let rpc_code = generator.generate();
    quote::quote! {
        #[::tari_rpc_framework::async_trait]
        #trait_code
        #rpc_code
    }
}

struct TraitInfoCollector {
    rpc_methods: Vec<RpcMethodInfo>,
    trait_ident: Option<syn::Ident>,
}

impl TraitInfoCollector {
    pub fn new() -> Self {
        Self {
            rpc_methods: Vec::new(),
            trait_ident: None,
        }
    }

    pub fn expect_trait_ident(&mut self) -> syn::Ident {
        self.trait_ident.take().unwrap()
    }

    /// Returns true if a method has the `#[rpc(...)]` attribute, otherwise false
    fn is_rpc_method(&self, node: &TraitItemFn) -> bool {
        node.attrs.iter().any(|at| at.meta.path().is_ident("rpc"))
    }

    fn parse_trait_item_method(&mut self, node: &mut TraitItemFn) -> syn::Result<RpcMethodInfo> {
        let mut info = RpcMethodInfo {
            method_ident: node.sig.ident.clone(),
            method_num: 0,
            is_server_streaming: false,
            request_type: None,
            return_type: None,
        };

        self.parse_attr(node, &mut info)?;
        self.parse_method_signature(node, &mut info)?;

        Ok(info)
    }

    fn parse_attr(&self, node: &mut TraitItemFn, info: &mut RpcMethodInfo) -> syn::Result<()> {
        let mut rpc_attrs = Vec::new();
        node.attrs.retain(|attr| {
            if attr.meta.path().is_ident("rpc") {
                rpc_attrs.push(attr.clone());
                false
            } else {
                true
            }
        });
        let Some(attr) = rpc_attrs.first() else {
            let ident = node.sig.ident.clone();
            return Err(syn_error!(node, "Missing #[rpc(...)] attribute on method `{}`", ident));
        };
        if let Some(extra) = rpc_attrs.get(1) {
            return Err(syn_error!(extra, "only one #[rpc(...)] attribute is allowed per method"));
        }

        let mut has_method = false;
        match &attr.meta {
            Meta::List(meta_list) => {
                let nested = meta_list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
                for meta in nested {
                    match meta {
                        Meta::NameValue(name_value) => {
                            let ident = name_value
                                .path
                                .get_ident()
                                .ok_or_else(|| syn_error!(&name_value.path, "expected identifier in #[rpc(...)]"))?;
                            match ident.to_string().as_str() {
                                "method" => {
                                    if has_method {
                                        return Err(syn_error!(ident, "`method` is specified more than once"));
                                    }
                                    has_method = true;
                                    info.method_num = extract_u32(ident, &name_value.value)?;
                                    self.validate_method_num(ident, info.method_num)?;
                                    if info.method_num == 0 {
                                        return Err(syn_error!(
                                            name_value,
                                            "method must be greater than 0 in `#[rpc(...)]` attribute for method `{}`",
                                            info.method_ident,
                                        ));
                                    }
                                },
                                s => {
                                    return Err(syn_error!(
                                        name_value,
                                        "Invalid option `{}` in #[rpc(...)] attribute",
                                        s
                                    ));
                                },
                            }
                        },
                        m => {
                            return Err(syn_error!(
                                m,
                                "Invalid syntax given to #[rpc(...)] attribute. Expected a name/value pair.",
                            ));
                        },
                    }
                }
            },
            m => {
                return Err(syn_error!(
                    m,
                    "Invalid syntax given to #[rpc(...)] attribute. Expected a name/value pair",
                ));
            },
        }

        if !has_method {
            return Err(syn_error!(
                attr,
                "#[rpc(...)] on method `{}` requires `method = <number>`",
                info.method_ident
            ));
        }

        Ok(())
    }

    fn validate_method_num<T: ToTokens>(&self, span: T, method_num: u32) -> Result<(), syn::Error> {
        if self.rpc_methods.iter().any(|m| m.method_num == method_num) {
            return Err(syn_error!(
                span,
                "duplicate method number `{}` in #[rpc(...]] attribute",
                method_num
            ));
        }

        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn parse_method_signature(&self, node: &TraitItemFn, info: &mut RpcMethodInfo) -> syn::Result<()> {
        info.method_ident = node.sig.ident.clone();

        // Check the self receiver
        let arg = node
            .sig
            .inputs
            .first()
            .ok_or_else(|| syn_error!(node, "RPC method `{}` has no arguments.", node.sig.ident))?;
        match arg {
            FnArg::Receiver(receiver) => {
                if receiver.reference.is_none() || receiver.mutability.is_some() {
                    return Err(syn_error!(receiver, "Method receiver must be an immutable reference",));
                }
            },
            _ => return Err(syn_error!(arg, "First argument is not a self receiver")),
        }

        if node.sig.inputs.len() != 2 {
            return Err(syn_error!(
                arg,
                "All RPC methods must take 2 arguments i.e `&self` and `request: Request<_>`.",
            ));
        }

        self.parse_request_type(node, info)?;
        self.parse_method_return_type(node, info)?;

        Ok(())
    }

    fn parse_method_return_type(&self, node: &TraitItemFn, info: &mut RpcMethodInfo) -> syn::Result<()> {
        let ident = info.method_ident.clone();
        let invalid_return_type = || {
            syn_error!(
                &node.sig.output,
                "Method `{}` has an invalid return type. Expected: `Result<_, RpcStatus>`",
                ident
            )
        };

        match &node.sig.output {
            ReturnType::Default => Err(invalid_return_type()),
            ReturnType::Type(_, ty) => match &**ty {
                Type::Path(path) => match path.path.segments.last() {
                    Some(syn::PathSegment {
                        ident,
                        arguments: syn::PathArguments::AngleBracketed(args),
                    }) if ident == "Result" => {
                        let arg = args.args.first().ok_or_else(invalid_return_type)?;
                        match arg {
                            GenericArgument::Type(Type::Path(syn::TypePath { path, .. })) => {
                                let ret_ty = path.segments.last().ok_or_else(invalid_return_type)?;
                                // Check if the response is streaming
                                match ret_ty.ident.to_string().as_str() {
                                    "Response" => {
                                        info.is_server_streaming = false;
                                    },
                                    "Streaming" => {
                                        info.is_server_streaming = true;
                                    },
                                    _ => return Err(invalid_return_type()),
                                }
                                // Store the return type
                                match &ret_ty.arguments {
                                    PathArguments::AngleBracketed(args) => {
                                        let arg = args.args.first().ok_or_else(invalid_return_type)?;
                                        match arg {
                                            GenericArgument::Type(ty) => {
                                                info.return_type = Some((*ty).clone());
                                                Ok(())
                                            },
                                            _ => Err(invalid_return_type()),
                                        }
                                    },
                                    _ => Err(invalid_return_type()),
                                }
                            },

                            _ => Err(invalid_return_type()),
                        }
                    },
                    _ => Err(invalid_return_type()),
                },
                _ => Err(invalid_return_type()),
            },
        }
    }

    fn parse_request_type(&self, node: &TraitItemFn, info: &mut RpcMethodInfo) -> syn::Result<()> {
        let request_arg = node
            .sig
            .inputs
            .get(1)
            .ok_or_else(|| syn_error!(&node.sig, "Missing request argument"))?;
        match request_arg {
            FnArg::Typed(syn::PatType { ty, .. }) => match &**ty {
                Type::Path(syn::TypePath { path, .. }) => {
                    let path = path
                        .segments
                        .last()
                        .filter(|seg| seg.ident == "Request")
                        .ok_or_else(|| syn_error!(request_arg, "expected Request<T>"))?;

                    match &path.arguments {
                        PathArguments::AngleBracketed(args) => {
                            let arg = args
                                .args
                                .first()
                                .ok_or_else(|| syn_error!(request_arg, "expected Request<T>"))?;
                            match arg {
                                GenericArgument::Type(ty) => {
                                    info.request_type = Some((*ty).clone());
                                    Ok(())
                                },
                                _ => Err(syn_error!(request_arg, "expected request type")),
                            }
                        },
                        _ => Err(syn_error!(request_arg, "expected request type")),
                    }
                },
                _ => Err(syn_error!(request_arg, "expected request type")),
            },
            _ => Err(syn_error!(request_arg, "expected request argument, got a receiver")),
        }
    }
}

impl Fold for TraitInfoCollector {
    fn fold_item_trait(&mut self, node: syn::ItemTrait) -> syn::ItemTrait {
        self.trait_ident = Some(node.ident.clone());
        fold::fold_item_trait(self, node)
    }

    fn fold_trait_item(&mut self, mut node: TraitItem) -> TraitItem {
        if let TraitItem::Fn(fn_item) = &mut node &&
            self.is_rpc_method(fn_item)
        {
            let info = match self.parse_trait_item_method(fn_item) {
                Ok(i) => i,
                Err(err) => {
                    panic!("{}", err);
                },
            };

            self.rpc_methods.push(info);
        }

        fold::fold_trait_item(self, node)
    }
}

fn extract_u32(ident: &syn::Ident, expr: &syn::Expr) -> syn::Result<u32> {
    match expr {
        syn::Expr::Lit(lit) => match &lit.lit {
            syn::Lit::Int(int) => int.base10_parse(),
            l => Err(syn_error!(
                ident,
                "Expected integer for `{}` in the #[rpc(...)] attribute, got {:?}",
                ident,
                l
            )),
        },
        _ => Err(syn_error!(
            expr,
            "Expected integer for `{}` in the #[rpc(...)] attribute, got {:?}",
            ident,
            expr
        )),
    }
}

#[cfg(test)]
mod tests {
    use syn::parse_quote;

    use super::*;

    fn parse_method(mut item: syn::TraitItemFn) -> syn::Result<RpcMethodInfo> {
        TraitInfoCollector::new().parse_trait_item_method(&mut item)
    }

    #[test]
    fn rejects_missing_method_key() {
        let item: syn::TraitItemFn = parse_quote! {
            #[rpc()]
            async fn foo(&self, req: Request<u32>) -> Result<Response<u32>, RpcStatus>;
        };
        let err = parse_method(item).unwrap_err();
        assert!(err.to_string().contains("requires `method = <number>`"), "{err}");
    }

    #[test]
    fn rejects_duplicate_rpc_attribute() {
        let item: syn::TraitItemFn = parse_quote! {
            #[rpc(method = 1)]
            #[rpc(method = 2)]
            async fn foo(&self, req: Request<u32>) -> Result<Response<u32>, RpcStatus>;
        };
        let err = parse_method(item).unwrap_err();
        assert!(
            err.to_string().contains("only one #[rpc(...)] attribute is allowed per method"),
            "{err}"
        );
    }

    #[test]
    fn rejects_duplicate_method_key() {
        let item: syn::TraitItemFn = parse_quote! {
            #[rpc(method = 1, method = 2)]
            async fn foo(&self, req: Request<u32>) -> Result<Response<u32>, RpcStatus>;
        };
        let err = parse_method(item).unwrap_err();
        assert!(err.to_string().contains("`method` is specified more than once"), "{err}");
    }

    #[test]
    fn rejects_by_value_self_receiver() {
        let item: syn::TraitItemFn = parse_quote! {
            #[rpc(method = 1)]
            async fn foo(self, req: Request<u32>) -> Result<Response<u32>, RpcStatus>;
        };
        let err = parse_method(item).unwrap_err();
        assert!(err.to_string().contains("Method receiver must be an immutable reference"), "{err}");
    }
}
