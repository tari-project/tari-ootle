//   Copyright 2022 The Tari Project
//   SPDX-License-Identifier: BSD-3-clause

use std::collections::HashMap;

use proc_macro2::Ident;
use tari_engine_types::substate::SubstateId;
use tari_ootle_transaction::{
    ComponentReference,
    Instruction,
    args::{InstructionArg, WorkspaceId, WorkspaceOffsetId},
    call_arg,
};
use tari_template_lib_types::{ComponentAddress, TemplateAddress, crypto::RistrettoPublicKeyBytes};

use crate::{
    ManifestInstructions,
    ManifestValue,
    ast::ManifestAst,
    error::ManifestError,
    parser::{InvokeIntent, ManifestIntent, ManifestLiteral, OrVar, OutputBinding, SpecialLiteral},
    value::lit_to_arg,
};

const MAX_CALL_DEPTH: usize = 16;

pub struct ManifestInstructionGenerator {
    imported_templates: HashMap<Ident, TemplateAddress>,
    global_aliases: HashMap<String, ManifestValue>,
    globals: HashMap<String, ManifestValue>,
    current_workspace_id: WorkspaceId,
    workspace_ids: HashMap<String, WorkspaceOffsetId>,
    templates: HashMap<String, TemplateAddress>,
    functions: HashMap<String, Vec<ManifestIntent>>,
    call_depth: usize,
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
            functions: HashMap::new(),
            call_depth: 0,
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

        self.functions = ast.parsed.functions;

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

    #[expect(clippy::too_many_lines)]
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
                    function: function_name
                        .to_string()
                        .try_into()
                        .map_err(|e| ManifestError::InvalidInstruction {
                            reason: format!("Function name is too long: {}", e),
                        })?,
                    args: self.process_args(arguments)?,
                }];
                if let Some(binding) = output_variable {
                    let key = self.current_workspace_id;
                    self.current_workspace_id += 1;
                    instructions.push(Instruction::PutLastInstructionOutputOnWorkspace { key });
                    self.register_output_binding(binding, key);
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
                let call = if let Some(value) = self.global_aliases.get(&component_ident) {
                    ComponentReference::Address(Self::extract_component_address(value)?)
                } else if let Some(workspace_offset) = self.workspace_ids.get(&component_ident) {
                    ComponentReference::Workspace(workspace_offset.id())
                } else if let Some(value) = self.globals.get(&component_ident) {
                    ComponentReference::Address(Self::extract_component_address(value)?)
                } else {
                    return Err(ManifestError::UndefinedVariable { name: component_ident });
                };
                let mut instructions = vec![Instruction::CallMethod {
                    call,
                    method: function_name
                        .to_string()
                        .try_into()
                        .map_err(|e| ManifestError::InvalidInstruction {
                            reason: format!("Method name is too long: {}", e),
                        })?,
                    args: self.process_args(arguments)?,
                }];
                if let Some(binding) = output_variable {
                    let key = self.current_workspace_id;
                    self.current_workspace_id += 1;
                    instructions.push(Instruction::PutLastInstructionOutputOnWorkspace { key });
                    self.register_output_binding(binding, key);
                }
                Ok(instructions)
            },
            ManifestIntent::AllocateAddress(alloc) => {
                let workspace_id = self.next_workspace_id(alloc.output_variable.to_string());
                Ok(vec![Instruction::AllocateAddress {
                    allocatable_type: alloc.allocatable_type,
                    workspace_id,
                }])
            },
            ManifestIntent::CreateAccount(create_account) => {
                let owner_public_key = self.extract_public_key(&create_account.owner_public_key.to_string())?;

                let owner_rule = create_account
                    .owner_rule
                    .map(|ident| self.extract_from_global(&ident.to_string()))
                    .transpose()?;

                let access_rules = create_account
                    .access_rules
                    .map(|ident| self.extract_from_global(&ident.to_string()))
                    .transpose()?;

                let bucket_workspace_id = create_account
                    .bucket
                    .map(|ident| {
                        let name = ident.to_string();
                        self.workspace_ids
                            .get(&name)
                            .copied()
                            .ok_or(ManifestError::UndefinedVariable { name })
                    })
                    .transpose()?;

                let mut instructions = vec![Instruction::CreateAccount {
                    owner_public_key,
                    owner_rule,
                    access_rules,
                    bucket_workspace_id,
                }];
                if let Some(var_name) = create_account.output_variable {
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
            ManifestIntent::CallLocalFunction(ident) => {
                if self.call_depth >= MAX_CALL_DEPTH {
                    return Err(ManifestError::MaxCallDepthExceeded { max: MAX_CALL_DEPTH });
                }
                let name = ident.to_string();
                let body = self
                    .functions
                    .get(&name)
                    .ok_or_else(|| ManifestError::UndefinedFunction { name: name.clone() })?
                    .clone();
                self.call_depth += 1;
                let mut instructions = Vec::with_capacity(body.len());
                for intent in body {
                    instructions.extend(self.translate_intent(intent)?);
                }
                self.call_depth -= 1;
                Ok(instructions)
            },
        }
    }

    fn next_workspace_id(&mut self, name: String) -> WorkspaceId {
        let id = self.current_workspace_id;
        self.workspace_ids.insert(name, WorkspaceOffsetId::new(id));
        self.current_workspace_id += 1;
        id
    }

    fn register_output_binding(&mut self, binding: OutputBinding, workspace_id: WorkspaceId) {
        match binding {
            OutputBinding::Single(ident) => {
                self.workspace_ids
                    .insert(ident.to_string(), WorkspaceOffsetId::new(workspace_id));
            },
            OutputBinding::Tuple(idents) => {
                for (i, ident) in idents.into_iter().enumerate() {
                    self.workspace_ids
                        .insert(ident.to_string(), WorkspaceOffsetId::new(workspace_id).with_offset(i));
                }
            },
        }
    }

    fn process_args(&self, args: Vec<ManifestLiteral>) -> Result<Vec<InstructionArg>, ManifestError> {
        args.into_iter()
            .map(|arg| match arg {
                ManifestLiteral::Lit(lit) => lit_to_arg(&lit),
                ManifestLiteral::Workspace(ident) => self.get_ident(&ident.to_string()),
                ManifestLiteral::Special(SpecialLiteral::Null) => {
                    Ok(InstructionArg::literal(tari_bor::Value::Null)
                        .expect("Null literal serialization should not fail"))
                },
                ManifestLiteral::Special(SpecialLiteral::Amount(amount)) => Ok(call_arg!(amount)),
                ManifestLiteral::Special(SpecialLiteral::NonFungibleId(id)) => Ok(call_arg!(id)),
                ManifestLiteral::Special(SpecialLiteral::Cbor(value)) => Ok(InstructionArg::literal(value)?),
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

    fn get_global(&self, name: &str) -> Result<&ManifestValue, ManifestError> {
        self.globals
            .get(name)
            .ok_or_else(|| ManifestError::UndefinedGlobal { name: name.to_string() })
    }

    fn extract_component_address(value: &ManifestValue) -> Result<ComponentAddress, ManifestError> {
        value
            .as_address()
            .and_then(|addr| addr.as_component_address())
            .ok_or_else(|| {
                ManifestError::InvalidVariableType(format!("Expected component variable but got {:?}", value))
            })
    }

    fn extract_public_key(&self, name: &str) -> Result<RistrettoPublicKeyBytes, ManifestError> {
        let value = self
            .global_aliases
            .get(name)
            .or_else(|| self.globals.get(name))
            .ok_or_else(|| ManifestError::UndefinedVariable { name: name.to_string() })?;

        match value {
            ManifestValue::Value(tari_bor::Value::Bytes(bytes)) => RistrettoPublicKeyBytes::from_bytes(bytes)
                .map_err(|e| ManifestError::InvalidVariableType(e.to_string())),
            _ => Err(ManifestError::InvalidVariableType(format!(
                "Expected public key bytes for variable '{name}' but got {value:?}"
            ))),
        }
    }

    fn extract_from_global<T: tari_bor::DeserializeOwned>(&self, name: &str) -> Result<T, ManifestError> {
        let value = self
            .global_aliases
            .get(name)
            .or_else(|| self.globals.get(name))
            .ok_or_else(|| ManifestError::UndefinedVariable { name: name.to_string() })?;

        match value {
            ManifestValue::Value(v) => tari_bor::from_value(v).map_err(|e| {
                ManifestError::InvalidVariableType(format!("Failed to deserialize variable '{name}': {e}"))
            }),
            _ => Err(ManifestError::InvalidVariableType(format!(
                "Expected serialized value for variable '{name}' but got {value:?}"
            ))),
        }
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
                    .map(|id| Ok(InstructionArg::Workspace(*id)))
            })
            .ok_or_else(|| {
                // Or undefined
                ManifestError::UndefinedVariable { name: name.to_string() }
            })?
    }
}
