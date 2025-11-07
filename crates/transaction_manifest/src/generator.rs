//   Copyright 2022 The Tari Project
//   SPDX-License-Identifier: BSD-3-clause

use std::collections::HashMap;

use proc_macro2::Ident;
use tari_engine_types::substate::SubstateId;
use tari_template_lib::types::TemplateAddress;
use tari_transaction::{
    args::{InstructionArg, WorkspaceId, WorkspaceOffsetId},
    call_arg,
    Instruction,
};

use crate::{
    ast::ManifestAst,
    error::ManifestError,
    parser::{InvokeIntent, ManifestIntent, ManifestLiteral, OrVar, SpecialLiteral},
    value::lit_to_arg,
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
                message: log.message.try_into().map_err(|e| ManifestError::InvalidInstruction {
                    reason: format!("Log message is too long: {}", e),
                })?,
            }]),
            ManifestIntent::DropAllProofs => Ok(vec![Instruction::DropAllProofsInWorkspace]),
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
                ManifestLiteral::Workspace(ident) => self.get_ident(&ident.to_string()),
                ManifestLiteral::Special(SpecialLiteral::Amount(amount)) => Ok(call_arg!(amount)),
                ManifestLiteral::Special(SpecialLiteral::NonFungibleId(id)) => Ok(call_arg!(id)),
                ManifestLiteral::Special(SpecialLiteral::Cbor(value)) => {
                    Ok(InstructionArg::literal(value).expect("CBOR literal serialization should not fail"))
                },
                ManifestLiteral::Special(SpecialLiteral::Metadata(metadata)) => Ok(call_arg!(metadata)),
                ManifestLiteral::Special(SpecialLiteral::SubstateId(id_or_var)) => match id_or_var {
                    OrVar::Var(ident) => self.get_ident(&ident.to_string()),
                    OrVar::Value(id) => Ok(call_arg!(id)),
                },
                ManifestLiteral::Special(SpecialLiteral::Address(var_or_id)) => match var_or_id {
                    OrVar::Var(ident) => self.get_ident(&ident.to_string()),
                    OrVar::Value(id) => match id {
                        SubstateId::Component(addr) => Ok(call_arg!(addr)),
                        SubstateId::Resource(addr) => Ok(call_arg!(addr)),
                        SubstateId::Vault(addr) => Ok(call_arg!(addr)),
                        SubstateId::ClaimedOutputTombstone(addr) => Ok(call_arg!(addr)),
                        SubstateId::NonFungible(addr) => Ok(call_arg!(addr)),
                        SubstateId::TransactionReceipt(addr) => Ok(call_arg!(addr)),
                        SubstateId::Template(addr) => Ok(call_arg!(addr)),
                        SubstateId::ValidatorFeePool(addr) => Ok(call_arg!(addr)),
                        SubstateId::Utxo(addr) => Ok(call_arg!(addr)),
                    },
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

    fn get_ident(&self, name: &str) -> Result<InstructionArg, ManifestError> {
        self.globals
            .get(name)
            .or_else(|| self.global_aliases.get(name))
            .map(|v| v.to_arg())
            .or_else(|| {
                // Or is it a variable on the worktop?
                self.workspace_ids
                    .get(name)
                    // TODO: support offsets
                    .map(|id| Ok(InstructionArg::Workspace(WorkspaceOffsetId::new(*id))))
            })
            .ok_or_else(|| {
                // Or undefined
                ManifestError::UndefinedVariable { name: name.to_string() }
            })?
    }
}
