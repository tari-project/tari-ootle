//   Copyright 2022 The Tari Project
//   SPDX-License-Identifier: BSD-3-clause

use std::collections::HashMap;

use proc_macro2::{Ident, TokenStream};
use syn::{
    Block,
    Expr,
    ExprCall,
    ExprLit,
    ExprMacro,
    ExprMethodCall,
    ExprPath,
    Item,
    ItemFn,
    ItemUse,
    Lit,
    LitStr,
    Local,
    Macro,
    Pat,
    PatIdent,
    Path,
    Signature,
    Stmt,
    UseTree,
    parse::ParseStream,
    parse2,
    punctuated::Punctuated,
    token::Comma,
};
use tari_engine_types::{json_cbor::convert_json_to_cbor, substate::SubstateId};
use tari_ootle_transaction::AllocatableAddressType;
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_template_lib::types::{
    Amount,
    LogLevel,
    Metadata,
    NonFungibleId,
    TemplateAddress,
    constants::TARI_TOKEN,
    hex::bytes_from_hex,
};

use crate::error::ManifestError;

#[derive(Debug, Clone)]
pub enum ManifestIntent {
    InvokeTemplate(InvokeIntent),
    InvokeComponent(InvokeIntent),
    AssignInput(AssignInputStmt),
    AllocateAddress(AllocateAddressStmt),
    Log(LogIntent),
    DropAllProofs,
    CallLocalFunction(Ident),
}

#[derive(Debug, Clone)]
pub struct ManifestImport {
    pub template_address: Option<TemplateAddress>,
    pub alias: Ident,
}

#[derive(Debug, Clone)]
pub struct InvokeIntent {
    pub output_variable: Option<Ident>,
    pub component_variable: Option<Ident>,
    pub template_variable: Option<Ident>,
    pub function_name: Ident,
    pub arguments: Vec<ManifestLiteral>,
}

#[derive(Debug, Clone)]
pub struct AssignInputStmt {
    pub variable_name: Ident,
    pub global_variable_name: LitStr,
}

#[derive(Debug, Clone)]
pub struct AllocateAddressStmt {
    pub output_variable: Ident,
    pub allocatable_type: AllocatableAddressType,
}

#[derive(Debug, Clone)]
pub struct LogIntent {
    pub level: LogLevel,
    pub message: String,
}

#[derive(Debug, Clone)]
pub enum ManifestLiteral {
    Lit(Lit),
    Workspace(Ident),
    Special(SpecialLiteral),
}

#[derive(Debug, Clone)]
pub enum SpecialLiteral {
    Amount(Amount),
    NonFungibleId(NonFungibleId),
    Cbor(tari_bor::Value),
    Metadata(Metadata),
    SubstateId(OrVar<SubstateId>),
    Address(OrVar<SubstateId>),
}

#[derive(Debug, Clone)]
pub enum OrVar<T> {
    Var(Ident),
    Value(T),
}

pub struct ManifestParser;

#[derive(Debug, Clone)]
pub struct ParsedManifest {
    pub defines: Vec<ManifestImport>,
    pub instruction_intents: Vec<ManifestIntent>,
    pub fee_instruction_intents: Vec<ManifestIntent>,
    pub functions: HashMap<String, Vec<ManifestIntent>>,
}

impl ManifestParser {
    pub fn new() -> Self {
        Self
    }

    pub fn parse(&self, input: ParseStream) -> Result<ParsedManifest, syn::Error> {
        let mut instruction_intents = vec![];
        let mut fee_instruction_intents = vec![];
        let mut functions = HashMap::new();
        let mut defines = vec![];
        defines.push(ManifestImport {
            template_address: Some(ACCOUNT_TEMPLATE_ADDRESS),
            alias: Ident::new("Account", proc_macro2::Span::call_site()),
        });

        for stmt in Block::parse_within(input)? {
            match stmt {
                // use template_hash as TemplateName;
                Stmt::Item(Item::Use(ItemUse {
                    tree: UseTree::Rename(rename),
                    ..
                })) => {
                    let template_id = rename.ident.to_string();
                    let template_address = template_id
                        .split_once('_')
                        .and_then(|(_, s)| TemplateAddress::from_hex(s).ok())
                        .ok_or_else(|| syn::Error::new_spanned(rename.clone(), "Invalid template address"))?;

                    defines.push(ManifestImport {
                        template_address: Some(template_address),
                        alias: rename.rename,
                    });
                },
                // use Name; // (predefined template)
                Stmt::Item(Item::Use(ItemUse {
                    tree: UseTree::Name(name),
                    ..
                })) => {
                    defines.push(ManifestImport {
                        template_address: None,
                        alias: name.ident,
                    });
                },
                Stmt::Item(Item::Fn(ItemFn {
                    block,
                    sig: Signature { ident, .. },
                    ..
                })) => {
                    if ident == "fee_main" {
                        fee_instruction_intents.extend(self.parse_block(*block)?);
                    } else if ident == "main" {
                        instruction_intents.extend(self.parse_block(*block)?);
                    } else {
                        let name = ident.to_string();
                        let body = self.parse_block(*block)?;
                        functions.insert(name, body);
                    }
                },
                _ => {
                    return Err(syn::Error::new_spanned(
                        stmt.clone(),
                        format!("Unsupported outer statement {:?}", stmt),
                    ));
                },
            }
        }

        Ok(ParsedManifest {
            defines,
            instruction_intents,
            fee_instruction_intents,
            functions,
        })
    }

    fn parse_block(&self, block: Block) -> Result<Vec<ManifestIntent>, syn::Error> {
        block.stmts.into_iter().map(|stmt| self.parse_stmt(stmt)).collect()
    }

    pub fn parse_stmt(&self, stmt: Stmt) -> Result<ManifestIntent, syn::Error> {
        match stmt {
            Stmt::Local(local) => self.handle_local(local),
            // component.function_name(arg1, arg2);
            Stmt::Expr(expr, _) => self.handle_semi_expr(expr),
            Stmt::Macro(mac) => self.handle_macro_stmt(mac),
            _ => Err(syn::Error::new_spanned(
                stmt.clone(),
                format!("Invalid statement {:?}", stmt),
            )),
        }
    }

    fn handle_macro_stmt(&self, mac: syn::StmtMacro) -> Result<ManifestIntent, syn::Error> {
        let Macro { path, tokens, .. } = mac.mac;

        let Some(mac_ident) = path.segments.first() else {
            return Err(syn::Error::new_spanned(path, "macro path must have a single segment"));
        };

        macro_call(&mac_ident.ident, tokens)
    }

    fn handle_local(&self, local: Local) -> Result<ManifestIntent, syn::Error> {
        // Parse let variable ident
        let var_ident = match local.pat {
            Pat::Ident(PatIdent { ref ident, .. }) => ident,
            // Pat::Tuple(pat) => return Ok(ManifestStmt::Todo),
            // Pat::Type(_) => {}
            // Pat::Macro(_) => {}
            // Pat::Reference(_) => {}
            // Pat::Slice(_) => {}
            // Pat::Struct(_) => {}
            // Pat::TupleStruct(_) => {}
            p => unimplemented!("{:?} not supported", p),
        };

        let expr = local.init.as_ref().map(|init| &init.expr).ok_or_else(|| {
            syn::Error::new_spanned(
                local.clone(),
                // I think this is `let x;`?
                "let expressions without an assignment are unsupported",
            )
        })?;

        let result = match *expr.clone() {
            Expr::Call(call) => {
                match &*call.func {
                    Expr::Path(path) => {
                        let mut iter = path.path.segments.iter();
                        let first = iter
                            .next()
                            .ok_or_else(|| syn::Error::new_spanned(path, "Invalid function call, empty path"))?;

                        if let Some(second) = iter.next() {
                            // Two segments: Template::function()
                            ManifestIntent::InvokeTemplate(InvokeIntent {
                                output_variable: Some(var_ident.clone()),
                                component_variable: None,
                                template_variable: Some(first.ident.clone()),
                                function_name: second.ident.clone(),
                                arguments: build_arguments(call.args)?,
                            })
                        } else {
                            // Single segment: local function call
                            ManifestIntent::CallLocalFunction(first.ident.clone())
                        }
                    },
                    _ => return Err(syn::Error::new_spanned(call.func, "Invalid function call")),
                }
            },
            Expr::MethodCall(ExprMethodCall {
                receiver, method, args, ..
            }) => {
                let receiver = extract_single_var_name(&receiver)?;
                ManifestIntent::InvokeComponent(InvokeIntent {
                    output_variable: Some(var_ident.clone()),
                    component_variable: Some(receiver),
                    template_variable: None,
                    function_name: method,
                    arguments: build_arguments(args)?,
                })
            },
            Expr::Macro(ExprMacro {
                mac: Macro { path, tokens, .. },
                ..
            }) => {
                if path.segments.len() != 1 {
                    // TODO: improve error
                    return Err(syn::Error::new_spanned(path, "macro path must have a single segment"));
                }

                assignment_from_macro(
                    var_ident.clone(),
                    &path
                        .segments
                        .first()
                        .expect("macro path must have a single segment")
                        .ident,
                    tokens,
                )?
            },
            _ => {
                return Err(syn::Error::new_spanned(
                    expr.clone(),
                    format!("Only function calls are supported in let statements. {:?}", expr),
                ));
            },
        };

        Ok(result)
    }

    fn handle_semi_expr(&self, expr: Expr) -> Result<ManifestIntent, syn::Error> {
        match expr {
            Expr::Call(call) => {
                match &*call.func {
                    Expr::Path(path) => {
                        let mut iter = path.path.segments.iter();
                        let first = iter
                            .next()
                            .ok_or_else(|| syn::Error::new_spanned(path, "Invalid function call, empty path"))?;

                        if let Some(second) = iter.next() {
                            // Two segments: Template::function()
                            Ok(ManifestIntent::InvokeTemplate(InvokeIntent {
                                output_variable: None,
                                component_variable: None,
                                template_variable: Some(first.ident.clone()),
                                function_name: second.ident.clone(),
                                arguments: build_arguments(call.args)?,
                            }))
                        } else {
                            // Single segment: local function call
                            Ok(ManifestIntent::CallLocalFunction(first.ident.clone()))
                        }
                    },
                    _ => return Err(syn::Error::new_spanned(call.func, "Invalid function call")),
                }
            },
            Expr::MethodCall(ExprMethodCall {
                receiver, method, args, ..
            }) => {
                let receiver = extract_single_var_name(&receiver)?;
                Ok(ManifestIntent::InvokeComponent(InvokeIntent {
                    output_variable: None,
                    component_variable: Some(receiver),
                    template_variable: None,
                    function_name: method,
                    arguments: build_arguments(args)?,
                }))
            },
            Expr::Macro(ExprMacro {
                mac: Macro { path, tokens, .. },
                ..
            }) => {
                let Some(mac) = path.segments.first() else {
                    return Err(syn::Error::new_spanned(path, "macro path must have a single segment"));
                };

                macro_call(&mac.ident, tokens)
            },
            _ => Err(syn::Error::new_spanned(
                expr.clone(),
                format!("Only function calls are supported in let statements. {:?}", expr),
            )),
        }
    }
}

fn assignment_from_macro(var_name: Ident, mac: &Ident, tokens: TokenStream) -> Result<ManifestIntent, syn::Error> {
    match mac.to_string().as_str() {
        "global" | "var" | "arg" => Ok(ManifestIntent::AssignInput(AssignInputStmt {
            variable_name: var_name,
            global_variable_name: parse2(tokens)?,
        })),
        "new_component_addr" => Ok(ManifestIntent::AllocateAddress(AllocateAddressStmt {
            output_variable: var_name,
            allocatable_type: AllocatableAddressType::Component,
        })),
        "new_resource_addr" => Ok(ManifestIntent::AllocateAddress(AllocateAddressStmt {
            output_variable: var_name,
            allocatable_type: AllocatableAddressType::Resource,
        })),
        _ => Err(syn::Error::new_spanned(mac, "Invalid macro name")),
    }
}

fn macro_call(mac: &Ident, tokens: TokenStream) -> Result<ManifestIntent, syn::Error> {
    match mac.to_string().as_str() {
        "info" => Ok(ManifestIntent::Log(LogIntent {
            level: LogLevel::Info,
            // TODO: Support format args - of course, this requires runtime support so is quite a heavy lift.
            message: parse2::<LitStr>(tokens)?.value(),
        })),
        "debug" => Ok(ManifestIntent::Log(LogIntent {
            level: LogLevel::Debug,
            message: parse2::<LitStr>(tokens)?.value(),
        })),
        "warn" => Ok(ManifestIntent::Log(LogIntent {
            level: LogLevel::Warn,
            message: parse2::<LitStr>(tokens)?.value(),
        })),
        "error" => Ok(ManifestIntent::Log(LogIntent {
            level: LogLevel::Error,
            message: parse2::<LitStr>(tokens)?.value(),
        })),
        "drop_all_proofs" => Ok(ManifestIntent::DropAllProofs),
        _ => Err(syn::Error::new_spanned(mac, "Invalid macro name")),
    }
}

fn build_arguments(args: Punctuated<Expr, Comma>) -> Result<Vec<ManifestLiteral>, syn::Error> {
    args.into_iter()
        .map(|arg| match arg {
            Expr::Lit(lit) => Ok(ManifestLiteral::Lit(lit.lit)),

            Expr::Path(ExprPath { path, .. }) => {
                if let Some(seg) = path.segments.first() {
                    if seg.ident == "XTR" || seg.ident == "TARI" {
                        Ok(ManifestLiteral::Special(SpecialLiteral::Address(OrVar::Value(
                            TARI_TOKEN.into(),
                        ))))
                    } else {
                        Ok(ManifestLiteral::Workspace(seg.ident.clone()))
                    }
                } else {
                    Err(syn::Error::new_spanned(
                        path,
                        "Invalid path, only single segment paths are supported",
                    ))
                }
            },
            // Support for 100 syntax
            Expr::Call(ExprCall { func, args, .. }) => {
                if let Expr::Path(ExprPath {
                    path: Path { segments, .. },
                    ..
                }) = &*func
                {
                    let name = segments
                        .first()
                        .ok_or_else(|| syn::Error::new_spanned(func.clone(), "Invalid function call"))?;

                    handle_special_literals(&name.ident, args)
                } else {
                    Err(syn::Error::new_spanned(
                        func,
                        "Invalid function call, only Amount is supported",
                    ))
                }
            },
            Expr::Macro(ExprMacro { mac, .. }) => match mac.path.get_ident() {
                Some(name) if name == "cbor" => {
                    let cbor_value: serde_json::Value = serde_json::from_str(&mac.tokens.to_string()).map_err(|e| {
                        syn::Error::new_spanned(&mac.tokens, format!("Failed to parse CBOR JSON value: {}", e))
                    })?;
                    let cbor = convert_json_to_cbor(cbor_value).map_err(|e| {
                        syn::Error::new_spanned(&mac.tokens, format!("Failed to convert JSON to CBOR value: {}", e))
                    })?;
                    Ok(ManifestLiteral::Special(SpecialLiteral::Cbor(cbor)))
                },
                _ => Err(syn::Error::new_spanned(
                    mac,
                    "Invalid argument, only literals and variables are supported",
                )),
            },
            _ => Err(syn::Error::new_spanned(
                arg,
                "Invalid argument, only literals and variables are supported",
            )),
        })
        .collect()
}

#[allow(clippy::too_many_lines)]
fn handle_special_literals(name: &Ident, args: Punctuated<Expr, Comma>) -> Result<ManifestLiteral, syn::Error> {
    let name_str = name.to_string();
    match name_str.as_str() {
        "Amount" => {
            let amt = args
                .first()
                .ok_or_else(|| syn::Error::new_spanned(name, "Invalid function call"))?;
            match amt {
                Expr::Lit(ExprLit { lit: Lit::Int(lit), .. }) => {
                    Ok(ManifestLiteral::Special(SpecialLiteral::Amount(lit.base10_parse()?)))
                },
                _ => Err(syn::Error::new_spanned(
                    amt,
                    "Invalid argument, only literals and variables are supported",
                )),
            }
        },
        "SubstateId" | "Address" => {
            let arg = args
                .first()
                .ok_or_else(|| syn::Error::new_spanned(name, "Invalid function call"))?;
            match arg {
                Expr::Lit(ExprLit {
                    lit: Lit::Str(lit_str), ..
                }) => {
                    let id = lit_str.value().parse().map_err(|e| {
                        syn::Error::new_spanned(lit_str, format!("Failed to parse Bytes from hex string: {}", e))
                    })?;
                    if name_str == "Address" {
                        Ok(ManifestLiteral::Special(SpecialLiteral::Address(OrVar::Value(id))))
                    } else {
                        Ok(ManifestLiteral::Special(SpecialLiteral::SubstateId(OrVar::Value(id))))
                    }
                },
                // TODO: more general support for this
                Expr::Path(ExprPath { path, .. }) => {
                    if let Some(seg) = path.segments.first() {
                        if name_str == "Address" {
                            Ok(ManifestLiteral::Special(SpecialLiteral::Address(OrVar::Var(
                                seg.ident.clone(),
                            ))))
                        } else {
                            Ok(ManifestLiteral::Special(SpecialLiteral::SubstateId(OrVar::Var(
                                seg.ident.clone(),
                            ))))
                        }
                    } else {
                        Err(syn::Error::new_spanned(
                            path,
                            "Invalid path, only single segment paths are supported",
                        ))
                    }
                },
                _ => Err(syn::Error::new_spanned(
                    arg,
                    "Invalid argument, only string literals are supported for Substate",
                )),
            }
        },
        "NonFungibleId" => {
            let arg = args
                .first()
                .ok_or_else(|| syn::Error::new_spanned(name, "Invalid function call"))?;
            if let Expr::Lit(ExprLit { lit, .. }) = arg {
                let id = lit_to_nonfungible_id(lit)
                    .map_err(|e| syn::Error::new_spanned(lit, format!("Failed to parse NonFungibleId: {}", e)))?;
                Ok(ManifestLiteral::Special(SpecialLiteral::NonFungibleId(id)))
            } else {
                Err(syn::Error::new_spanned(
                    arg,
                    "Invalid argument, only literals and variables are supported",
                ))
            }
        },
        "Metadata" => {
            let arg = args
                .first()
                .ok_or_else(|| syn::Error::new_spanned(name, "Invalid function call"))?;
            if let Expr::Lit(ExprLit {
                lit: Lit::Str(lit_str), ..
            }) = arg
            {
                let metadata: Metadata = lit_str.value().parse().map_err(|e| {
                    syn::Error::new_spanned(lit_str, format!("Failed to parse Metadata JSON value: {}", e))
                })?;
                Ok(ManifestLiteral::Special(SpecialLiteral::Metadata(metadata)))
            } else {
                Err(syn::Error::new_spanned(
                    arg,
                    "Invalid argument, only string literals are supported for Metadata",
                ))
            }
        },
        "HexBytes" | "PublicKey" => {
            let arg = args
                .first()
                .ok_or_else(|| syn::Error::new_spanned(name, "Invalid function call"))?;
            if let Expr::Lit(ExprLit {
                lit: Lit::Str(lit_str), ..
            }) = arg
            {
                let bytes = bytes_from_hex(&lit_str.value()).map_err(|e| {
                    syn::Error::new_spanned(lit_str, format!("Failed to parse Bytes from hex string: {}", e))
                })?;
                Ok(ManifestLiteral::Special(SpecialLiteral::Cbor(tari_bor::Value::Bytes(
                    bytes,
                ))))
            } else {
                Err(syn::Error::new_spanned(
                    arg,
                    "Invalid argument, only string literals are supported for Bytes",
                ))
            }
        },
        "Cbor" => {
            let expr = args
                .first()
                .ok_or_else(|| syn::Error::new_spanned(name, "Invalid function call"))?;
            match expr {
                Expr::Lit(ExprLit { lit: Lit::Str(lit), .. }) => {
                    let cbor_value: serde_json::Value = serde_json::from_str(&lit.value())
                        .map_err(|e| syn::Error::new_spanned(lit, format!("Failed to parse CBOR JSON value: {}", e)))?;
                    let cbor = convert_json_to_cbor(cbor_value).map_err(|e| {
                        syn::Error::new_spanned(lit, format!("Failed to convert JSON to CBOR value: {}", e))
                    })?;
                    Ok(ManifestLiteral::Special(SpecialLiteral::Cbor(cbor)))
                },
                _ => Err(syn::Error::new_spanned(
                    expr,
                    "Invalid argument, only string literals are supported",
                )),
            }
        },
        s => Err(syn::Error::new_spanned(
            name,
            format!(
                "Invalid function call '{s}', only Amount, SubstateId, Address, NonFungibleId, Metadata, HexBytes, \
                 Cbor and PublicKey are supported"
            ),
        )),
    }
}

fn extract_single_var_name(expr: &Expr) -> Result<Ident, syn::Error> {
    match expr {
        Expr::Path(ExprPath {
            path: Path { segments, .. },
            ..
        }) => {
            if segments.len() != 1 {
                return Err(syn::Error::new_spanned(expr, "Invalid method call"));
            }
            Ok(segments[0].ident.clone())
        },
        _ => Err(syn::Error::new_spanned(
            expr.clone(),
            format!("Invalid method call {:?}", expr),
        )),
    }
}

fn lit_to_nonfungible_id(lit: &Lit) -> Result<NonFungibleId, ManifestError> {
    match lit {
        Lit::Str(s) => Ok(NonFungibleId::try_from_string(s.value()).map_err(|e| {
            ManifestError::UnsupportedExpr(format!(
                "Invalid non-fungible ID string literal ({:?}) ({})",
                e,
                s.value()
            ))
        })?),
        Lit::ByteStr(v) => {
            let bytes = v.value();
            if bytes.len() != 32 {
                return Err(ManifestError::UnsupportedExpr(
                    "Non-fungible ID byte string literal length must be less than 32 bytes".to_string(),
                ));
            }

            let mut id = [0u8; 32];
            id.copy_from_slice(&bytes);
            Ok(NonFungibleId::from_u256(id))
        },
        Lit::Int(v) => match v.suffix() {
            "u8" | "u16" | "u32" => Ok(NonFungibleId::from_u32(v.base10_parse()?)),
            "u64" => Ok(NonFungibleId::from_u64(v.base10_parse()?)),
            "" => Err(ManifestError::UnsupportedExpr(
                "Non-fungible ID integer literal must have a type suffix specified (1u32, 2u64 etc)".to_string(),
            )),
            _ => Err(ManifestError::UnsupportedExpr(format!(
                "Invalid non-fungible ID integer literal suffix ({})",
                v.suffix()
            ))),
        },
        _ => Err(ManifestError::UnsupportedExpr(format!(
            "Unsupported non-fungible ID literal ({:?})",
            lit
        ))),
    }
}
