//  Copyright 2023, The Tari Project
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

use std::{
    convert::{TryFrom, TryInto},
    num::TryFromIntError,
};

use anyhow::{anyhow, Context};
use tari_engine_types::{
    confidential::{AbridgedTransactionKernel, ClaimBurnOutputData, EncodedMerkleProof, MinotariBurnClaimProof},
    substate::SubstateId,
};
use tari_ootle_common_types::{SubstateRequirement, SubstateRequirementRef, VersionedSubstateId};
use tari_template_lib::{
    auth::OwnerRule,
    models::{
        ComponentAddress,
        ConfidentialOutputStatement,
        ConfidentialWithdrawProof,
        StealthInput,
        UnspentOutput,
        ViewableBalanceProof,
    },
    prelude::AccessRules,
    types::{
        crypto::{BalanceProofSignature, PedersenCommitmentBytes, RangeProofBytes, RistrettoPublicKeyBytes, UtxoTag},
        EncryptedData,
        ObjectKey,
    },
};
use tari_transaction::{
    args::{InstructionArg, WorkspaceId, WorkspaceOffsetId},
    AllocatableAddressType,
    ComponentCall,
    Instruction,
    ResourceAddressRef,
    Transaction,
};

use crate::{
    encoding::{decode_from_slice, encode_to_vec},
    proto::{self, transaction::OptionalVersion},
    utils::checked_copy_fixed,
    NewTransactionMessage,
};
// -------------------------------- NewTransactionMessage -------------------------------- //

impl From<NewTransactionMessage> for proto::transaction::NewTransactionMessage {
    fn from(msg: NewTransactionMessage) -> Self {
        Self {
            transaction: Some((&msg.transaction).into()),
        }
    }
}

impl TryFrom<proto::transaction::NewTransactionMessage> for NewTransactionMessage {
    type Error = anyhow::Error;

    fn try_from(value: proto::transaction::NewTransactionMessage) -> Result<Self, Self::Error> {
        Ok(NewTransactionMessage {
            transaction: value
                .transaction
                .ok_or_else(|| anyhow!("Transaction not provided"))?
                .try_into()?,
        })
    }
}

//---------------------------------- Transaction --------------------------------------------//
impl TryFrom<proto::transaction::Transaction> for Transaction {
    type Error = anyhow::Error;

    fn try_from(transaction: proto::transaction::Transaction) -> Result<Self, Self::Error> {
        decode_from_slice(&transaction.bor_encoded)
    }
}

impl From<&Transaction> for proto::transaction::Transaction {
    fn from(transaction: &Transaction) -> Self {
        proto::transaction::Transaction {
            // TODO: no panic
            bor_encoded: encode_to_vec(transaction).expect("Failed to encode transaction"),
        }
    }
}

// -------------------------------- AllocatableAddressType -------------------------------- //

impl TryFrom<proto::transaction::AllocatableAddressType> for AllocatableAddressType {
    type Error = anyhow::Error;

    fn try_from(substate_type: proto::transaction::AllocatableAddressType) -> Result<Self, Self::Error> {
        match substate_type {
            proto::transaction::AllocatableAddressType::None => {
                anyhow::bail!("AllocatableAddressType not provided");
            },
            proto::transaction::AllocatableAddressType::Component => Ok(AllocatableAddressType::Component),
            proto::transaction::AllocatableAddressType::Resource => Ok(AllocatableAddressType::Resource),
        }
    }
}

impl From<AllocatableAddressType> for proto::transaction::AllocatableAddressType {
    fn from(substate_type: AllocatableAddressType) -> Self {
        match substate_type {
            AllocatableAddressType::Component => proto::transaction::AllocatableAddressType::Component,
            AllocatableAddressType::Resource => proto::transaction::AllocatableAddressType::Resource,
        }
    }
}

// -------------------------------- Instruction -------------------------------- //

impl TryFrom<proto::transaction::Instruction> for Instruction {
    type Error = anyhow::Error;

    #[allow(clippy::too_many_lines)]
    fn try_from(value: proto::transaction::Instruction) -> Result<Self, Self::Error> {
        use proto::transaction::instruction::Instruction::*;
        match value.instruction {
            None => Err(anyhow!("instruction not provided")),
            Some(CallFunction(call_function)) => Ok(Instruction::CallFunction {
                address: call_function.template_address.try_into()?,
                function: call_function.function,
                args: call_function
                    .args
                    .into_iter()
                    .map(|a| a.try_into())
                    .collect::<Result<_, _>>()?,
            }),
            Some(CallMethod(call_method)) => Ok(Instruction::CallMethod {
                call: call_method
                    .component_call
                    .ok_or_else(|| anyhow!("component_call not provided"))?
                    .try_into()?,
                method: call_method.method,
                args: call_method
                    .args
                    .into_iter()
                    .map(|a| a.try_into())
                    .collect::<Result<_, _>>()?,
            }),
            Some(PutLastInstructionOutputOnWorkspace(id)) => Ok(Instruction::PutLastInstructionOutputOnWorkspace {
                key: u16::try_from(id).context("workspace_put_key overflowed")?,
            }),
            Some(EmitLog(emit_log)) => Ok(Instruction::EmitLog {
                level: emit_log.log_level.parse()?,
                message: emit_log
                    .log_message
                    .try_into()
                    .map_err(|e| anyhow!("emit_log_message: {}", e))?,
            }),
            Some(ClaimBurn(claim_burn)) => Ok(Instruction::ClaimBurn {
                claim: Box::new(MinotariBurnClaimProof {
                    burn_public_key: claim_burn
                        .public_key
                        .as_slice()
                        .try_into()
                        .map_err(|e| anyhow!("claim_burn_public_key: {}", e))?,
                    commitment: claim_burn
                        .commitment
                        .as_slice()
                        .try_into()
                        .map_err(|e| anyhow!("claim_burn_commitment_address: {}", e))?,
                    ownership_proof: claim_burn
                        .proof_of_knowledge
                        .ok_or_else(|| anyhow!("claim_burn_proof_of_knowledge not provided"))?
                        .try_into()
                        .map_err(|e| anyhow!("claim_burn_proof_of_knowledge: {}", e))?,
                    encoded_merkle_proof: claim_burn
                        .encoded_merkle_proof
                        .ok_or_else(|| anyhow!("claim_burn_encoded_merkle_proof not provided"))?
                        .try_into()?,
                    kernel: claim_burn
                        .kernel
                        .ok_or_else(|| anyhow!("claim_burn_kernel not provided"))?
                        .try_into()?,
                    value: claim_burn.value,
                }),
                output_data: ClaimBurnOutputData {
                    encrypted_data: EncryptedData::try_from(claim_burn.encrypted_data)
                        .map_err(|len| anyhow!("Invalid length ({len}) of encrypted_value bytes"))?,
                },
            }),
            Some(ClaimValidatorFees(claim_fees)) => Ok(Instruction::ClaimValidatorFees {
                address: claim_fees
                    .fee_pool_address
                    .as_slice()
                    .try_into()
                    .map_err(|e| anyhow!("claim_validator_fees_address: {e}"))?,
            }),
            Some(DropAllProofsInWorkspace(_)) => Ok(Instruction::DropAllProofsInWorkspace),
            Some(CreateAccount(create_account)) => Ok(Instruction::CreateAccount {
                owner_public_key: RistrettoPublicKeyBytes::from_bytes(&create_account.public_key)
                    .map_err(|e| anyhow!("create_account_public_key: {}", e))?,
                owner_rule: create_account.owner_rule.map(TryInto::try_into).transpose()?,
                access_rules: create_account.access_rules.map(TryInto::try_into).transpose()?,
                workspace_id: create_account
                    .workspace_id
                    .map(|offset_id| {
                        let offset = offset_id.offset.map(|o| usize::try_from(o.offset)).transpose()?;
                        let id = WorkspaceId::try_from(offset_id.id)?;
                        Ok::<_, TryFromIntError>(WorkspaceOffsetId::new(id).with_offset_opt(offset))
                    })
                    .transpose()
                    .context("create_account_workspace_id overflowed")?,
            }),
            Some(AssertBucketContains(assert_contains)) => {
                let resource_address = ObjectKey::try_from(assert_contains.resource_address)?.into();
                Ok(Instruction::AssertBucketContains {
                    key: assert_contains
                        .bucket
                        .ok_or_else(|| anyhow!("assert_bucket_workspace_id not provided"))?
                        .try_into()?,
                    resource_address,
                    min_amount: assert_contains.min_amount.unwrap_or_default().into(),
                })
            },
            Some(TakeFromBucket(take_from_bucket)) => Ok(Instruction::TakeFromBucket {
                input_bucket: take_from_bucket
                    .input_bucket
                    .ok_or_else(|| anyhow!("take_from_bucket_input_bucket not provided"))?
                    .try_into()?,
                amount: take_from_bucket.amount.unwrap_or_default().into(),
                output_bucket: u16::try_from(take_from_bucket.output_bucket)
                    .context("take_from_bucket_output_bucket overflowed")?,
            }),
            Some(PublishTemplate(publish_template)) => Ok(Instruction::PublishTemplate {
                binary: publish_template.binary,
            }),
            Some(AllocateAddress(allocate_addr)) => {
                let address_type = allocate_addr.address_type();
                Ok(Instruction::AllocateAddress {
                    allocatable_type: address_type
                        .try_into()
                        .map_err(|e| anyhow!("invalid allocatable_address_type {e}"))?,
                    workspace_id: WorkspaceId::try_from(allocate_addr.workspace_id)
                        .context("allocate_address_workspace_id overflowed")?,
                })
            },
            Some(StealthTransfer(transfer)) => Ok(Instruction::StealthTransfer {
                resource_address_ref: transfer
                    .resource_address
                    .map(TryInto::try_into)
                    .transpose()?
                    .ok_or_else(|| anyhow!("stealth_transfer_resource_address not provided"))?,
                statement: transfer
                    .statement
                    .ok_or_else(|| anyhow!("stealth_transfer_statement not provided"))?
                    .try_into()
                    .context("stealth_transfer_statement conversion failed")?,
                revealed_input_bucket: transfer.revealed_input_bucket.map(TryInto::try_into).transpose()?,
            }),
            Some(PayFee(pay_fee)) => Ok(Instruction::PayFee {
                statement: pay_fee
                    .statement
                    .ok_or_else(|| anyhow!("pay_fee_stealth_transfer_statement not provided"))?
                    .try_into()
                    .context("pay_fee_stealth_transfer_statement conversion failed")?,
                revealed_input_bucket: pay_fee.revealed_input_bucket.map(TryInto::try_into).transpose()?,
            }),
        }
    }
}

impl From<Instruction> for proto::transaction::Instruction {
    #[allow(clippy::too_many_lines)]
    fn from(instruction: Instruction) -> Self {
        match instruction {
            Instruction::CreateAccount {
                owner_public_key,
                owner_rule,
                access_rules,
                workspace_id,
            } => {
                let owner_rule = owner_rule.map(Into::into);
                let access_rules = access_rules.map(Into::into);
                let workspace_id = workspace_id.map(Into::into);
                proto::transaction::Instruction {
                    instruction: Some(proto::transaction::instruction::Instruction::CreateAccount(
                        proto::transaction::CreateAccount {
                            public_key: owner_public_key.as_bytes().to_vec(),
                            owner_rule,
                            access_rules,
                            workspace_id,
                        },
                    )),
                }
            },
            Instruction::CallFunction {
                address,
                function,
                args,
            } => {
                let args = args.into_iter().map(Into::into).collect();
                proto::transaction::Instruction {
                    instruction: Some(proto::transaction::instruction::Instruction::CallFunction(
                        proto::transaction::CallFunction {
                            template_address: address.as_slice().to_vec(),
                            function,
                            args,
                        },
                    )),
                }
            },
            Instruction::CallMethod { call, method, args } => {
                let call = Some(call.into());
                let args = args.into_iter().map(Into::into).collect();
                proto::transaction::Instruction {
                    instruction: Some(proto::transaction::instruction::Instruction::CallMethod(
                        proto::transaction::CallMethod {
                            component_call: call,
                            method,
                            args,
                        },
                    )),
                }
            },
            Instruction::PutLastInstructionOutputOnWorkspace { key } => proto::transaction::Instruction {
                instruction: Some(
                    proto::transaction::instruction::Instruction::PutLastInstructionOutputOnWorkspace(key.into()),
                ),
            },
            Instruction::EmitLog { level, message } => proto::transaction::Instruction {
                instruction: Some(proto::transaction::instruction::Instruction::EmitLog(
                    proto::transaction::EmitLog {
                        log_level: level.to_string(),
                        log_message: message.into_string(),
                    },
                )),
            },
            Instruction::ClaimBurn { claim, output_data } => proto::transaction::Instruction {
                instruction: Some(proto::transaction::instruction::Instruction::ClaimBurn(
                    proto::transaction::ClaimBurn {
                        public_key: claim.burn_public_key.as_bytes().to_vec(),
                        commitment: claim.commitment.as_bytes().to_vec(),
                        proof_of_knowledge: Some(claim.ownership_proof.into()),
                        encoded_merkle_proof: Some(claim.encoded_merkle_proof.into()),
                        kernel: Some(claim.kernel.into()),
                        value: claim.value,
                        encrypted_data: output_data.encrypted_data.as_bytes().to_vec(),
                    },
                )),
            },
            Instruction::ClaimValidatorFees { address } => proto::transaction::Instruction {
                instruction: Some(proto::transaction::instruction::Instruction::ClaimValidatorFees(
                    proto::transaction::ClaimValidatorFees {
                        fee_pool_address: address.as_slice().to_vec(),
                    },
                )),
            },
            Instruction::DropAllProofsInWorkspace => proto::transaction::Instruction {
                instruction: Some(proto::transaction::instruction::Instruction::DropAllProofsInWorkspace(
                    true,
                )),
            },
            Instruction::AssertBucketContains {
                key,
                resource_address,
                min_amount,
            } => proto::transaction::Instruction {
                instruction: Some(proto::transaction::instruction::Instruction::AssertBucketContains(
                    proto::transaction::AssertBucketContains {
                        bucket: Some(key.into()),
                        resource_address: resource_address.as_bytes().to_vec(),
                        min_amount: Some(min_amount.into()),
                    },
                )),
            },

            Instruction::TakeFromBucket {
                input_bucket,
                amount,
                output_bucket,
            } => proto::transaction::Instruction {
                instruction: Some(proto::transaction::instruction::Instruction::TakeFromBucket(
                    proto::transaction::TakeFromBucket {
                        input_bucket: Some(input_bucket.into()),
                        amount: Some(amount.into()),
                        output_bucket: u32::from(output_bucket),
                    },
                )),
            },
            Instruction::PublishTemplate { binary } => proto::transaction::Instruction {
                instruction: Some(proto::transaction::instruction::Instruction::PublishTemplate(
                    proto::transaction::PublishTemplate { binary },
                )),
            },
            Instruction::AllocateAddress {
                allocatable_type,
                workspace_id,
            } => proto::transaction::Instruction {
                instruction: Some(proto::transaction::instruction::Instruction::AllocateAddress(
                    proto::transaction::AllocateAddress {
                        address_type: proto::transaction::AllocatableAddressType::from(allocatable_type).into(),
                        workspace_id: u32::from(workspace_id),
                    },
                )),
            },
            Instruction::StealthTransfer {
                resource_address_ref,
                statement,
                revealed_input_bucket,
            } => {
                let revealed_input_bucket = revealed_input_bucket.map(Into::into);
                proto::transaction::Instruction {
                    instruction: Some(proto::transaction::instruction::Instruction::StealthTransfer(
                        proto::transaction::StealthTransfer {
                            resource_address: Some(resource_address_ref.into()),
                            statement: Some(statement.into()),
                            revealed_input_bucket,
                        },
                    )),
                }
            },
            Instruction::PayFee {
                statement,
                revealed_input_bucket,
            } => {
                let revealed_input_bucket = revealed_input_bucket.map(Into::into);
                proto::transaction::Instruction {
                    instruction: Some(proto::transaction::instruction::Instruction::PayFee(
                        proto::transaction::PayFee {
                            statement: Some(statement.into()),
                            revealed_input_bucket,
                        },
                    )),
                }
            },
        }
    }
}

// -------------------------------- Arg -------------------------------- //

impl TryFrom<proto::transaction::Arg> for InstructionArg {
    type Error = anyhow::Error;

    fn try_from(request: proto::transaction::Arg) -> Result<Self, Self::Error> {
        let arg_value = request.arg_value.ok_or_else(|| anyhow!("arg_value not provided"))?;
        match arg_value {
            proto::transaction::arg::ArgValue::Literal(data) => Ok(InstructionArg::raw_literal_bytes(data)),
            proto::transaction::arg::ArgValue::Workspace(offset_id) => {
                let id = u16::try_from(offset_id.id).context("WorkspaceOffsetId id overflowed")?;
                Ok(InstructionArg::Workspace(
                    WorkspaceOffsetId::new(id).with_offset_opt(offset_id.offset.map(|o| o.offset as usize)),
                ))
            },
        }
    }
}

impl From<InstructionArg> for proto::transaction::Arg {
    fn from(arg: InstructionArg) -> Self {
        match arg {
            InstructionArg::Literal(data) => proto::transaction::Arg {
                arg_value: Some(proto::transaction::arg::ArgValue::Literal(data.into())),
            },
            InstructionArg::Workspace(offset_id) => proto::transaction::Arg {
                arg_value: Some(proto::transaction::arg::ArgValue::Workspace(
                    proto::transaction::WorkspaceOffsetId {
                        id: u32::from(offset_id.id()),
                        offset: offset_id
                            .offset()
                            .map(|o| proto::transaction::OptionOffset { offset: o as u64 }),
                    },
                )),
            },
        }
    }
}

// -------------------------------- ComponentCall -------------------------------- //
impl TryFrom<proto::transaction::ComponentCall> for ComponentCall {
    type Error = anyhow::Error;

    fn try_from(value: proto::transaction::ComponentCall) -> Result<Self, Self::Error> {
        match value.call {
            Some(proto::transaction::component_call::Call::Address(address)) => {
                let address = ComponentAddress::from(ObjectKey::try_from(address)?);
                Ok(ComponentCall::Address(address))
            },
            Some(proto::transaction::component_call::Call::Allocation(key)) => Ok(ComponentCall::Workspace(
                key.try_into().context("ComponentCall::Allocation key overflowed")?,
            )),
            None => Err(anyhow!("ComponentCall must have a call specified")),
        }
    }
}

impl From<ComponentCall> for proto::transaction::ComponentCall {
    fn from(call: ComponentCall) -> Self {
        match call {
            ComponentCall::Address(address) => proto::transaction::ComponentCall {
                call: Some(proto::transaction::component_call::Call::Address(
                    address.as_bytes().to_vec(),
                )),
            },
            ComponentCall::Workspace(id) => proto::transaction::ComponentCall {
                call: Some(proto::transaction::component_call::Call::Allocation(u32::from(id))),
            },
        }
    }
}

// -------------------------------- ResourceAddressRef -------------------------------- //
impl TryFrom<proto::transaction::ResourceAddressRef> for ResourceAddressRef {
    type Error = anyhow::Error;

    fn try_from(val: proto::transaction::ResourceAddressRef) -> Result<Self, Self::Error> {
        let inner = val
            .inner
            .ok_or_else(|| anyhow!("ResourceAddressRef inner not provided"))?;
        match inner {
            proto::transaction::resource_address_ref::Inner::Address(address) => {
                Ok(ResourceAddressRef::Address(ObjectKey::try_from_slice(&address)?.into()))
            },
            proto::transaction::resource_address_ref::Inner::Workspace(workspace_id) => {
                Ok(ResourceAddressRef::Workspace(
                    workspace_id
                        .try_into()
                        .context("ResourceAddressRef::Workspace conversion failed")?,
                ))
            },
        }
    }
}

impl From<ResourceAddressRef> for proto::transaction::ResourceAddressRef {
    fn from(val: ResourceAddressRef) -> Self {
        match val {
            ResourceAddressRef::Address(address) => proto::transaction::ResourceAddressRef {
                inner: Some(proto::transaction::resource_address_ref::Inner::Address(
                    address.as_bytes().to_vec(),
                )),
            },
            ResourceAddressRef::Workspace(workspace_id) => proto::transaction::ResourceAddressRef {
                inner: Some(proto::transaction::resource_address_ref::Inner::Workspace(
                    proto::transaction::WorkspaceOffsetId {
                        id: u32::from(workspace_id.id()),
                        offset: workspace_id
                            .offset()
                            .map(|o| proto::transaction::OptionOffset { offset: o as u64 }),
                    },
                )),
            },
        }
    }
}

// -------------------------------- SubstateRequirement -------------------------------- //
impl TryFrom<proto::transaction::SubstateRequirement> for SubstateRequirement {
    type Error = anyhow::Error;

    fn try_from(val: proto::transaction::SubstateRequirement) -> Result<Self, Self::Error> {
        let substate_id = SubstateId::from_bytes(&val.substate_id)?;
        let version = val.version.map(|v| v.version);
        let substate_specification = SubstateRequirement::new(substate_id, version);
        Ok(substate_specification)
    }
}

impl From<SubstateRequirement> for proto::transaction::SubstateRequirement {
    fn from(val: SubstateRequirement) -> Self {
        (&val).into()
    }
}

impl From<&SubstateRequirement> for proto::transaction::SubstateRequirement {
    fn from(val: &SubstateRequirement) -> Self {
        Self {
            substate_id: val.substate_id().to_bytes(),
            version: val.version().map(|v| OptionalVersion { version: v }),
        }
    }
}
impl From<SubstateRequirementRef<'_>> for proto::transaction::SubstateRequirement {
    fn from(val: SubstateRequirementRef<'_>) -> Self {
        Self {
            substate_id: val.substate_id().to_bytes(),
            version: val.version().map(|v| OptionalVersion { version: v }),
        }
    }
}

// -------------------------------- VersionedSubstate -------------------------------- //

impl TryFrom<proto::transaction::VersionedSubstateId> for VersionedSubstateId {
    type Error = anyhow::Error;

    fn try_from(val: proto::transaction::VersionedSubstateId) -> Result<Self, Self::Error> {
        let substate_id = SubstateId::from_bytes(&val.substate_id)?;
        let substate_specification = VersionedSubstateId::new(substate_id, val.version);
        Ok(substate_specification)
    }
}

impl From<VersionedSubstateId> for proto::transaction::VersionedSubstateId {
    fn from(val: VersionedSubstateId) -> Self {
        (&val).into()
    }
}

impl From<&VersionedSubstateId> for proto::transaction::VersionedSubstateId {
    fn from(val: &VersionedSubstateId) -> Self {
        Self {
            substate_id: val.substate_id().to_bytes(),
            version: val.version(),
        }
    }
}

// // -------------------------------- ConfidentialWithdrawProof -------------------------------- //

impl TryFrom<proto::transaction::ConfidentialWithdrawProof> for ConfidentialWithdrawProof {
    type Error = anyhow::Error;

    fn try_from(val: proto::transaction::ConfidentialWithdrawProof) -> Result<Self, Self::Error> {
        Ok(ConfidentialWithdrawProof {
            inputs: val
                .inputs
                .into_iter()
                .map(|v| {
                    PedersenCommitmentBytes::from_bytes(&v).map_err(|e| anyhow!("Invalid input commitment bytes: {e}"))
                })
                .collect::<Result<_, _>>()?,
            input_revealed_amount: val.input_revealed_amount.unwrap_or_default().into(),
            output_proof: val
                .output_proof
                .ok_or_else(|| anyhow!("output_proof is missing"))?
                .try_into()?,
            balance_proof: BalanceProofSignature::from_bytes(&val.balance_proof)
                .map_err(|e| anyhow!("Invalid balance proof signature: {}", e))?,
        })
    }
}

impl From<ConfidentialWithdrawProof> for proto::transaction::ConfidentialWithdrawProof {
    fn from(val: ConfidentialWithdrawProof) -> Self {
        Self {
            inputs: val.inputs.iter().map(|v| v.as_bytes().to_vec()).collect(),
            input_revealed_amount: Some(val.input_revealed_amount.into()),
            output_proof: Some(val.output_proof.into()),
            balance_proof: val.balance_proof.to_bytes(),
        }
    }
}

// -------------------------------- ConfidentialOutputStatement -------------------------------- //

impl TryFrom<proto::transaction::ConfidentialOutputStatement> for ConfidentialOutputStatement {
    type Error = anyhow::Error;

    fn try_from(val: proto::transaction::ConfidentialOutputStatement) -> Result<Self, Self::Error> {
        Ok(ConfidentialOutputStatement {
            output: val.output_statement.map(TryInto::try_into).transpose()?,
            change_statement: val.change_statement.map(TryInto::try_into).transpose()?,
            range_proof: RangeProofBytes::try_from(val.range_proof).context("Invalid range proof")?,
            output_revealed_amount: val.output_revealed_amount.unwrap_or_default().into(),
            change_revealed_amount: val.change_revealed_amount.unwrap_or_default().into(),
        })
    }
}

impl From<ConfidentialOutputStatement> for proto::transaction::ConfidentialOutputStatement {
    fn from(val: ConfidentialOutputStatement) -> Self {
        Self {
            output_statement: val.output.map(Into::into),
            change_statement: val.change_statement.map(Into::into),
            range_proof: val.range_proof.into_vec(),
            output_revealed_amount: Some(val.output_revealed_amount.into()),
            change_revealed_amount: Some(val.change_revealed_amount.into()),
        }
    }
}

// -------------------------------- UnspentOutput -------------------------------- //

impl TryFrom<proto::transaction::UnspentOutput> for UnspentOutput {
    type Error = anyhow::Error;

    fn try_from(val: proto::transaction::UnspentOutput) -> Result<Self, Self::Error> {
        let sender_public_nonce = Some(val.sender_public_nonce)
            .filter(|v| !v.is_empty())
            .map(|v| RistrettoPublicKeyBytes::from_bytes(&v).map_err(|e| anyhow!("Invalid sender_public_nonce: {}", e)))
            .transpose()?
            .ok_or_else(|| anyhow!("sender_public_nonce is missing"))?;

        Ok(UnspentOutput {
            commitment: checked_copy_fixed(&val.commitment)
                .ok_or_else(|| anyhow!("Invalid length of commitment bytes"))?,
            sender_public_nonce,
            encrypted_data: EncryptedData::try_from(val.encrypted_data)
                .map_err(|len| anyhow!("Invalid length ({len}) of encrypted_value bytes"))?,
            minimum_value_promise: val.minimum_value_promise,
            viewable_balance_proof: val.viewable_balance_proof.map(TryInto::try_into).transpose()?,
        })
    }
}

impl From<UnspentOutput> for proto::transaction::UnspentOutput {
    fn from(val: UnspentOutput) -> Self {
        Self {
            commitment: val.commitment.to_vec(),
            sender_public_nonce: val.sender_public_nonce.as_bytes().to_vec(),
            encrypted_data: val.encrypted_data.as_ref().to_vec(),
            minimum_value_promise: val.minimum_value_promise,
            viewable_balance_proof: val.viewable_balance_proof.map(Into::into),
        }
    }
}

// -------------------------------- StealthUnspentOutput -------------------------------- //
impl TryFrom<proto::transaction::StealthUnspentOutput> for tari_template_lib::models::StealthUnspentOutput {
    type Error = anyhow::Error;

    fn try_from(val: proto::transaction::StealthUnspentOutput) -> Result<Self, Self::Error> {
        let output = val
            .output
            .ok_or_else(|| anyhow!("stealth unspent output not provided"))?
            .try_into()?;
        let owner_public_key = RistrettoPublicKeyBytes::from_bytes(&val.owner_public_key)
            .map_err(|e| anyhow!("Invalid owner public key: {}", e))?;

        Ok(tari_template_lib::models::StealthUnspentOutput {
            output,
            owner_public_key,
            tag: UtxoTag::new(val.tag_byte),
        })
    }
}

impl From<tari_template_lib::models::StealthUnspentOutput> for proto::transaction::StealthUnspentOutput {
    fn from(val: tari_template_lib::models::StealthUnspentOutput) -> Self {
        Self {
            output: Some(val.output.into()),
            owner_public_key: val.owner_public_key.as_bytes().to_vec(),
            tag_byte: val.tag.value(),
        }
    }
}

// -------------------------------- ViewableBalanceProof -------------------------------- //

impl TryFrom<proto::transaction::ViewableBalanceProof> for ViewableBalanceProof {
    type Error = anyhow::Error;

    fn try_from(val: proto::transaction::ViewableBalanceProof) -> Result<Self, Self::Error> {
        Ok(ViewableBalanceProof {
            elgamal_encrypted: val.elgamal_encrypted.as_slice().try_into()?,
            elgamal_public_nonce: val.elgamal_public_nonce.as_slice().try_into()?,
            c_prime: val.c_prime.as_slice().try_into()?,
            e_prime: val.e_prime.as_slice().try_into()?,
            r_prime: val.r_prime.as_slice().try_into()?,
            s_v: val.s_v.as_slice().try_into()?,
            s_m: val.s_m.as_slice().try_into()?,
            s_r: val.s_r.as_slice().try_into()?,
        })
    }
}

impl From<ViewableBalanceProof> for proto::transaction::ViewableBalanceProof {
    fn from(val: ViewableBalanceProof) -> Self {
        Self {
            elgamal_encrypted: val.elgamal_encrypted.as_bytes().to_vec(),
            elgamal_public_nonce: val.elgamal_public_nonce.as_bytes().to_vec(),
            c_prime: val.c_prime.as_bytes().to_vec(),
            e_prime: val.e_prime.as_bytes().to_vec(),
            r_prime: val.r_prime.as_bytes().to_vec(),
            s_v: val.s_v.as_bytes().to_vec(),
            s_m: val.s_m.as_bytes().to_vec(),
            s_r: val.s_r.as_bytes().to_vec(),
        }
    }
}

//---------------------------------- StealthTransferStatement --------------------------------------------//

impl TryFrom<proto::transaction::StealthTransferStatement> for tari_template_lib::models::StealthTransferStatement {
    type Error = anyhow::Error;

    fn try_from(value: proto::transaction::StealthTransferStatement) -> Result<Self, Self::Error> {
        Ok(Self {
            inputs_statement: value
                .inputs_statement
                .ok_or_else(|| anyhow!("Inputs statement not provided"))?
                .try_into()?,
            outputs_statement: value
                .outputs_statement
                .ok_or_else(|| anyhow!("output_statement not provided"))?
                .try_into()?,
            balance_proof: Some(&value.balance_proof)
                .filter(|b| !b.is_empty())
                .map(|b| BalanceProofSignature::from_bytes(b))
                .transpose()
                .context("Invalid balance proof signature")?,
        })
    }
}

impl From<tari_template_lib::models::StealthTransferStatement> for proto::transaction::StealthTransferStatement {
    fn from(value: tari_template_lib::models::StealthTransferStatement) -> Self {
        Self {
            inputs_statement: Some(value.inputs_statement.into()),
            outputs_statement: Some(value.outputs_statement.into()),
            balance_proof: value.balance_proof.as_ref().map(|b| b.to_bytes()).unwrap_or_default(),
        }
    }
}

// -------------------------------- StealthInputsStatement -------------------------------- //
impl TryFrom<proto::transaction::StealthInputsStatement> for tari_template_lib::models::StealthInputsStatement {
    type Error = anyhow::Error;

    fn try_from(value: proto::transaction::StealthInputsStatement) -> Result<Self, Self::Error> {
        let inputs = value
            .inputs
            .into_iter()
            .map(|input| input.try_into().context("Invalid stealth input"))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            inputs,
            revealed_amount: value.revealed_amount.unwrap_or_default().into(),
            required_signer: RistrettoPublicKeyBytes::from_bytes(&value.required_signer)
                .context("Invalid required signer")?,
        })
    }
}

impl From<tari_template_lib::models::StealthInputsStatement> for proto::transaction::StealthInputsStatement {
    fn from(value: tari_template_lib::models::StealthInputsStatement) -> Self {
        Self {
            inputs: value.inputs.iter().map(Into::into).collect(),
            revealed_amount: Some(value.revealed_amount.into()),
            required_signer: value.required_signer.as_bytes().to_vec(),
        }
    }
}

// -------------------------------- StealthInput -------------------------------- //

impl TryFrom<proto::transaction::StealthInput> for StealthInput {
    type Error = anyhow::Error;

    fn try_from(value: proto::transaction::StealthInput) -> Result<Self, Self::Error> {
        let commitment =
            PedersenCommitmentBytes::from_bytes(&value.commitment).context("Invalid input commitment bytes")?;
        let owner_proof = value
            .owner_proof
            .ok_or_else(|| anyhow!("owner_proof not provided"))?
            .try_into()
            .context("Invalid owner proof commitment signature")?;

        Ok(Self {
            commitment,
            owner_proof,
        })
    }
}

impl From<&StealthInput> for proto::transaction::StealthInput {
    fn from(value: &StealthInput) -> Self {
        Self {
            commitment: value.commitment.as_bytes().to_vec(),
            owner_proof: Some((&value.owner_proof).into()),
        }
    }
}

//---------------------------------- StealthOutputStatement --------------------------------------------//

impl TryFrom<proto::transaction::StealthOutputsStatement> for tari_template_lib::models::StealthOutputsStatement {
    type Error = anyhow::Error;

    fn try_from(value: proto::transaction::StealthOutputsStatement) -> Result<Self, Self::Error> {
        let outputs = value
            .outputs
            .into_iter()
            .map(|output| output.try_into().context("Invalid unspent output"))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            outputs,
            revealed_output_amount: value.revealed_output_amount.unwrap_or_default().into(),
            agg_range_proof: RangeProofBytes::try_from(value.agg_range_proof)
                .context("Invalid aggregate range proof")?,
        })
    }
}

impl From<tari_template_lib::models::StealthOutputsStatement> for proto::transaction::StealthOutputsStatement {
    fn from(value: tari_template_lib::models::StealthOutputsStatement) -> Self {
        Self {
            outputs: value.outputs.into_iter().map(Into::into).collect(),
            revealed_output_amount: Some(value.revealed_output_amount.into()),
            agg_range_proof: value.agg_range_proof.into_vec(),
        }
    }
}

// -------------------------------- OwnerRule -------------------------------- //

impl From<OwnerRule> for proto::transaction::OwnerRule {
    fn from(value: OwnerRule) -> Self {
        Self {
            // TODO: no panic
            encoded_owner_rule: encode_to_vec(&value).unwrap(),
        }
    }
}

impl TryFrom<proto::transaction::OwnerRule> for OwnerRule {
    type Error = anyhow::Error;

    fn try_from(value: proto::transaction::OwnerRule) -> Result<Self, Self::Error> {
        decode_from_slice(&value.encoded_owner_rule)
    }
}

// -------------------------------- AccessRules -------------------------------- //

impl From<AccessRules> for proto::transaction::AccessRules {
    fn from(value: AccessRules) -> Self {
        Self {
            // TODO: no panic
            encoded_access_rules: encode_to_vec(&value).unwrap(),
        }
    }
}

impl TryFrom<proto::transaction::AccessRules> for AccessRules {
    type Error = anyhow::Error;

    fn try_from(value: proto::transaction::AccessRules) -> Result<Self, Self::Error> {
        decode_from_slice(&value.encoded_access_rules)
    }
}

// -------------------------------- WorkspaceOffsetId -------------------------------- //

impl TryFrom<proto::transaction::WorkspaceOffsetId> for WorkspaceOffsetId {
    type Error = anyhow::Error;

    fn try_from(value: proto::transaction::WorkspaceOffsetId) -> Result<Self, Self::Error> {
        let id = WorkspaceId::try_from(value.id).context("WorkspaceOffsetId id overflowed")?;
        let offset = value
            .offset
            .map(|o| usize::try_from(o.offset).context("WorkspaceOffsetId offset overflowed"))
            .transpose()?;
        Ok(Self::new(id).with_offset_opt(offset))
    }
}

impl From<WorkspaceOffsetId> for proto::transaction::WorkspaceOffsetId {
    fn from(value: WorkspaceOffsetId) -> Self {
        Self {
            id: u32::from(value.id()),
            offset: value.offset().map(|o| proto::transaction::OptionOffset {
                offset: u64::try_from(o).expect("WorkspaceOffsetId offset overflowed"),
            }),
        }
    }
}

// -------------------------------- EncodedMerkleProof -------------------------------- //

impl TryFrom<proto::transaction::EncodedMerkleProof> for EncodedMerkleProof {
    type Error = anyhow::Error;

    fn try_from(value: proto::transaction::EncodedMerkleProof) -> Result<Self, Self::Error> {
        Ok(Self {
            block_hash: value.block_hash.try_into().context("Invalid block hash")?,
            encoded_merkle_proof: value
                .encoded_proof
                .try_into()
                .map_err(|e| anyhow!("Invalid encoded merkle proof: {}", e))?,
            leaf_index: value.leaf_index,
        })
    }
}

impl From<EncodedMerkleProof> for proto::transaction::EncodedMerkleProof {
    fn from(value: EncodedMerkleProof) -> Self {
        Self {
            block_hash: value.block_hash.as_slice().to_vec(),
            encoded_proof: value.encoded_merkle_proof.to_vec(),
            leaf_index: value.leaf_index,
        }
    }
}

// -------------------------------- AbridgedKernel -------------------------------- //

impl TryFrom<proto::transaction::AbridgedKernel> for AbridgedTransactionKernel {
    type Error = anyhow::Error;

    fn try_from(value: proto::transaction::AbridgedKernel) -> Result<Self, Self::Error> {
        Ok(Self {
            version: u8::try_from(value.version).map_err(|e| anyhow!("Invalid kernel version: {}", e))?,
            fee: value.fee,
            lock_height: value.lock_height,
            excess: PedersenCommitmentBytes::from_bytes(&value.excess)
                .map_err(|e| anyhow!("Invalid excess commitment: {}", e))?,
            excess_sig: value
                .excess_sig
                .ok_or_else(|| anyhow!("excess_sig not provided"))?
                .try_into()
                .map_err(|e| anyhow!("Invalid excess signature: {}", e))?,
        })
    }
}

impl From<AbridgedTransactionKernel> for proto::transaction::AbridgedKernel {
    fn from(value: AbridgedTransactionKernel) -> Self {
        Self {
            version: value.version.into(),
            fee: value.fee,
            lock_height: value.lock_height,
            excess: value.excess.as_bytes().to_vec(),
            excess_sig: Some(value.excess_sig.into()),
        }
    }
}
