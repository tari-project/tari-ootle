//   Copyright 2022 The Tari Project
//   SPDX-License-Identifier: BSD-3-clause

use std::collections::HashMap;

use proc_macro2::Ident;
use syn::Lit;
use tari_engine_types::substate::SubstateId;
use tari_template_lib::{models::NonFungibleId, types::TemplateAddress};
use tari_transaction::{
    args::{InstructionArg, WorkspaceId, WorkspaceOffsetId},
    call_arg,
    Instruction,
};

use crate::{
    ast::ManifestAst,
    error::ManifestError,
    parser::{InvokeIntent, ManifestIntent, ManifestLiteral, SpecialLiteral},
    ManifestInstructions,
    ManifestValue,
};

pub struct ManifestInstructionGenerator {
    imported_templates: HashMap<Ident, TemplateAddress>,
    global_aliases: HashMap<String, ManifestValue>,
    globals: HashMap<String, ManifestValue>,
    current_workspace_id: WorkspaceId,
    workspace_ids: HashMap<String, WorkspaceId>,
    templates: HashMap<String, TemplateAddress>,
}

impl ManifestInstructionGenerator {
    pub fn new(globals: HashMap<String, ManifestValue>, templates: HashMap<String, TemplateAddress>) -> Self {
        Self {
            imported_templates: HashMap::new(),
            global_aliases: HashMap::new(),
            globals,
            current_workspace_id: WorkspaceId::default(),
            workspace_ids: HashMap::new(),
            templates,
        }
    }

    pub fn generate_instructions(&mut self, ast: ManifestAst) -> Result<ManifestInstructions, ManifestError> {
        self.imported_templates = ast
            .parsed
            .defines
            .into_iter()
            .map(|import| match import.template_address {
                Some(addr) => Ok((import.alias, addr)),
                None => {
                    let alias_str = import.alias.to_string();
                    self.templates
                        .get(&alias_str)
                        .copied()
                        .map(|addr| (import.alias, addr))
                        .ok_or(ManifestError::TemplateAliasNotDefined { alias: alias_str })
                },
            })
            .collect::<Result<_, _>>()?;

        let mut instructions = Vec::with_capacity(ast.parsed.instruction_intents.len());
        for intent in ast.parsed.instruction_intents {
            instructions.extend(self.translate_intent(intent)?);
        }

        let mut fee_instructions = Vec::with_capacity(ast.parsed.fee_instruction_intents.len());
        for intent in ast.parsed.fee_instruction_intents {
            fee_instructions.extend(self.translate_intent(intent)?);
        }

        Ok(ManifestInstructions {
            instructions,
            fee_instructions,
        })
    }

    fn translate_intent(&mut self, intent: ManifestIntent) -> Result<Vec<Instruction>, ManifestError> {
        match intent {
            ManifestIntent::InvokeTemplate(InvokeIntent {
                output_variable,
                template_variable,
                function_name,
                arguments,
                ..
            }) => {
                let template_ident = template_variable
                    .as_ref()
                    .expect("AST parse should have failed: no template ident for TemplateInvoke statement");
                let mut instructions = vec![Instruction::CallFunction {
                    address: self.get_imported_template(template_ident)?,
                    function: function_name.to_string(),
                    args: self.process_args(arguments)?,
                }];
                if let Some(var_name) = output_variable {
                    let key = self.next_workspace_id(var_name.to_string());
                    instructions.push(Instruction::PutLastInstructionOutputOnWorkspace { key });
                }
                Ok(instructions)
            },
            ManifestIntent::InvokeComponent(InvokeIntent {
                output_variable,
                component_variable,
                function_name,
                arguments,
                ..
            }) => {
                let component_ident = component_variable
                    .as_ref()
                    .expect("AST parse should have failed: no component ident for ComponentInvoke statement")
                    .to_string();
                let component_address = self
                    .get_variable(&component_ident)?
                    .as_address()
                    .and_then(|addr| addr.as_component_address())
                    .ok_or_else(|| {
                        ManifestError::InvalidVariableType(format!(
                            "Expected component variable but got {:?}",
                            self.get_variable(&component_ident)
                        ))
                    })?;
                let mut instructions = vec![Instruction::CallMethod {
                    call: component_address.into(),
                    method: function_name.to_string(),
                    args: self.process_args(arguments)?,
                }];
                if let Some(var_name) = output_variable {
                    let key = self.next_workspace_id(var_name.to_string());
                    instructions.push(Instruction::PutLastInstructionOutputOnWorkspace { key });
                }
                Ok(instructions)
            },
            ManifestIntent::AssignInput(assign) => {
                self.global_aliases.insert(
                    assign.variable_name.to_string(),
                    self.get_global(&assign.global_variable_name.value())?.clone(),
                );
                Ok(vec![])
            },
            ManifestIntent::Log(log) => Ok(vec![Instruction::EmitLog {
                level: log.level,
                message: log.message,
            }]),
        }
    }

    fn next_workspace_id(&mut self, name: String) -> WorkspaceId {
        let id = self.current_workspace_id;
        self.workspace_ids.insert(name, id);
        self.current_workspace_id += 1;
        id
    }

    fn process_args(&self, args: Vec<ManifestLiteral>) -> Result<Vec<InstructionArg>, ManifestError> {
        args.into_iter()
            .map(|arg| match arg {
                ManifestLiteral::Lit(lit) => lit_to_arg(&lit),
                ManifestLiteral::Workspace(ident) => {
                    // Is it a global?
                    self.globals
                        .get(&ident.to_string())
                        .or_else(|| self.global_aliases.get(&ident.to_string()))
                        .map(|v| match v {
                            ManifestValue::SubstateId(addr) => match addr {
                                SubstateId::Component(addr) => Ok(call_arg!(*addr)),
                                SubstateId::Resource(addr) => Ok(call_arg!(*addr)),
                                // TODO: should tx receipt addresses be allowed to be referenced?
                                SubstateId::TransactionReceipt(addr) => Ok(call_arg!(*addr)),
                                SubstateId::Vault(addr) => Ok(call_arg!(*addr)),
                                SubstateId::NonFungible(addr) => Ok(call_arg!(addr)),
                                SubstateId::ClaimedOutputTombstone(addr) => Ok(call_arg!(*addr)),
                                SubstateId::Template(addr) => Ok(call_arg!(*addr)),
                                SubstateId::ValidatorFeePool(addr) => Ok(call_arg!(*addr)),
                                SubstateId::Utxo(addr) => Ok(call_arg!(*addr)),
                            },
                            ManifestValue::Literal(lit) => lit_to_arg(lit),
                            ManifestValue::NonFungibleId(id) => Ok(call_arg!(id.clone())),
                            ManifestValue::Value(blob) => Ok(InstructionArg::literal(blob.clone()).unwrap()),
                        })
                        .or_else(|| {
                            // Or is it a variable on the worktop?
                            self.workspace_ids
                                .get(&ident.to_string())
                                // TODO: support offsets
                                .map(|id| Ok(InstructionArg::Workspace(WorkspaceOffsetId::new(*id))))
                        })
                        .ok_or_else(|| {
                            // Or undefined
                            ManifestError::UndefinedVariable {
                                name: ident.to_string(),
                            }
                        })?
                },
                ManifestLiteral::Special(SpecialLiteral::Amount(amount)) => Ok(call_arg!(amount)),
                ManifestLiteral::Special(SpecialLiteral::NonFungibleId(lit)) => {
                    let id = lit_to_nonfungible_id(&lit)?;
                    Ok(call_arg!(id))
                },
            })
            .collect()
    }

    fn get_imported_template(&self, name: &Ident) -> Result<TemplateAddress, ManifestError> {
        self.imported_templates
            .get(name)
            .copied()
            .ok_or_else(|| ManifestError::TemplateNotImported { name: name.to_string() })
    }

    fn get_variable(&self, name: &str) -> Result<&ManifestValue, ManifestError> {
        self.global_aliases
            .get(name)
            .ok_or_else(|| ManifestError::UndefinedVariable { name: name.to_string() })
    }

    fn get_global(&self, name: &str) -> Result<&ManifestValue, ManifestError> {
        self.globals
            .get(name)
            .ok_or_else(|| ManifestError::UndefinedGlobal { name: name.to_string() })
    }
}

fn lit_to_arg(lit: &Lit) -> Result<InstructionArg, ManifestError> {
    match lit {
        Lit::Str(s) => Ok(call_arg!(s.value())),
        Lit::Int(i) => match i.suffix() {
            "u8" => Ok(call_arg!(i.base10_parse::<u8>()?)),
            "u16" => Ok(call_arg!(i.base10_parse::<u16>()?)),
            "u32" => Ok(call_arg!(i.base10_parse::<u32>()?)),
            "u64" => Ok(call_arg!(i.base10_parse::<u64>()?)),
            "u128" => Ok(call_arg!(i.base10_parse::<u128>()?)),
            "i8" => Ok(call_arg!(i.base10_parse::<i8>()?)),
            "i16" => Ok(call_arg!(i.base10_parse::<i16>()?)),
            "i32" => Ok(call_arg!(i.base10_parse::<i32>()?)),
            "i64" => Ok(call_arg!(i.base10_parse::<i64>()?)),
            "" | "i128" => Ok(call_arg!(i.base10_parse::<i128>()?)),
            _ => Err(ManifestError::UnsupportedExpr(format!(
                r#"Unsupported integer suffix "{}""#,
                i.suffix()
            ))),
        },
        Lit::Bool(b) => Ok(call_arg!(b.value())),
        Lit::ByteStr(v) => Ok(call_arg!(v.value())),
        Lit::Byte(v) => Ok(call_arg!(v.value())),
        Lit::Char(v) => Ok(call_arg!(v.value().to_string())),
        Lit::Float(v) => Err(ManifestError::UnsupportedExpr(format!(
            "Float literals not supported ({})",
            v
        ))),
        Lit::Verbatim(v) => Err(ManifestError::UnsupportedExpr(format!(
            "Raw token literals not supported ({})",
            v
        ))),
        _ => Err(ManifestError::UnsupportedExpr(format!(
            "Unsupported literal type ({:?})",
            lit
        ))),
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
