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
    confidential::ConfidentialClaim,
    instruction::Instruction,
    substate::SubstateId,
    ComponentCall,
    ResourceAddressRef,
};
use tari_ootle_common_types::{SubstateRequirement, SubstateRequirementRef, VersionedSubstateId};
use tari_template_lib::{
    args::{AllocatableAddressType, InstructionArg, WorkspaceId, WorkspaceOffsetId},
    auth::OwnerRule,
    models::{
        ComponentAddress,
        ConfidentialOutputStatement,
        ConfidentialWithdrawProof,
        EncryptedData,
        StealthInput,
        UnspentOutput,
        ViewableBalanceProof,
    },
    prelude::{AccessRules, Scalar32Bytes},
    types::{
        crypto::{
            BalanceProofSignature,
            CommitmentSignatureBytes,
            PedersenCommitmentBytes,
            RangeProofBytes,
            RistrettoPublicKeyBytes,
        },
        ObjectKey,
    },
};
use tari_transaction::Transaction;

use crate::{
    encoding::{decode_from_slice, encode_to_vec},
    proto::{
        self,
        transaction::{instruction::InstructionType, OptionalVersion},
    },
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

//---------------------------------- UnsignedTransaction --------------------------------------------//

// impl TryFrom<proto::transaction::UnsignedTransactionV1> for UnsignedTransactionV1 {
//     type Error = anyhow::Error;
//
//     fn try_from(request: proto::transaction::UnsignedTransactionV1) -> Result<Self, Self::Error> {
//         let instructions = request
//             .instructions
//             .into_iter()
//             .map(TryInto::try_into)
//             .collect::<Result<Vec<_>, _>>()?;
//
//         let fee_instructions = request
//             .fee_instructions
//             .into_iter()
//             .map(TryInto::try_into)
//             .collect::<Result<Vec<_>, _>>()?;
//
//         let inputs = request
//             .inputs
//             .into_iter()
//             .map(TryInto::try_into)
//             .collect::<Result<_, _>>()?;
//
//         let min_epoch = request.min_epoch.map(|epoch| Epoch(epoch.epoch));
//         let max_epoch = request.max_epoch.map(|epoch| Epoch(epoch.epoch));
//         Ok(Self {
//             fee_instructions,
//             instructions,
//             inputs,
//             min_epoch,
//             max_epoch,
//         })
//     }
// }
//
// impl From<&UnsignedTransactionV1> for proto::transaction::UnsignedTransaction {
//     fn from(transaction: &UnsignedTransactionV1) -> Self {
//         let inputs = transaction.inputs().iter().map(Into::into).collect();
//         let min_epoch = transaction
//             .min_epoch()
//             .map(|epoch| proto::common::Epoch { epoch: epoch.0 });
//         let max_epoch = transaction
//             .max_epoch()
//             .map(|epoch| proto::common::Epoch { epoch: epoch.0 });
//         let fee_instructions = transaction.fee_instructions().iter().cloned().map(Into::into).collect();
//         let instructions = transaction.instructions().iter().cloned().map(Into::into).collect();
//
//         proto::transaction::UnsignedTransaction {
//             fee_instructions,
//             instructions,
//             inputs,
//             min_epoch,
//             max_epoch,
//         }
//     }
// }

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
    fn try_from(request: proto::transaction::Instruction) -> Result<Self, Self::Error> {
        let substate_type = request.allocatable_address_type();
        let args = request
            .args
            .into_iter()
            .map(|a| a.try_into())
            .collect::<Result<_, _>>()?;
        let instruction_type =
            InstructionType::try_from(request.instruction_type).map_err(|e| anyhow!("invalid instruction_type {e}"))?;

        let instruction = match instruction_type {
            InstructionType::CreateAccount => Instruction::CreateAccount {
                public_key_address: RistrettoPublicKeyBytes::from_bytes(&request.create_account_public_key)
                    .map_err(|e| anyhow!("create_account_public_key: {}", e))?,
                owner_rule: request.create_account_owner_rule.map(TryInto::try_into).transpose()?,
                access_rules: request.create_account_access_rules.map(TryInto::try_into).transpose()?,
                workspace_id: request
                    .create_account_workspace_id
                    .map(|offset_id| {
                        let offset = offset_id.offset.map(|o| usize::try_from(o.offset)).transpose()?;
                        let id = WorkspaceId::try_from(offset_id.id)?;
                        Ok::<_, TryFromIntError>(WorkspaceOffsetId::new(id).with_offset_opt(offset))
                    })
                    .transpose()
                    .context("create_account_workspace_id overflowed")?,
            },
            InstructionType::Function => {
                let function = request.function;
                Instruction::CallFunction {
                    address: request.template_address.try_into()?,
                    function,
                    args,
                }
            },
            InstructionType::Method => {
                let method = request.method;
                let call = request
                    .component_call
                    .ok_or_else(|| anyhow!("component_call not provided"))?
                    .try_into()?;
                Instruction::CallMethod { call, method, args }
            },
            InstructionType::PutOutputInWorkspace => Instruction::PutLastInstructionOutputOnWorkspace {
                key: u16::try_from(request.workspace_put_key).context("workspace_put_key overflowed")?,
            },
            InstructionType::EmitLog => Instruction::EmitLog {
                level: request.log_level.parse()?,
                message: request.log_message,
            },
            InstructionType::ClaimBurn => Instruction::ClaimBurn {
                claim: Box::new(ConfidentialClaim {
                    public_key: request
                        .claim_burn_public_key
                        .as_slice()
                        .try_into()
                        .map_err(|e| anyhow!("claim_burn_public_key: {}", e))?,
                    output_address: request
                        .claim_burn_commitment_address
                        .as_slice()
                        .try_into()
                        .map_err(|e| anyhow!("claim_burn_commitment_address: {}", e))?,
                    range_proof: RangeProofBytes::try_from(request.claim_burn_range_proof)
                        .context("invalid range proof")?,
                    proof_of_knowledge: request
                        .claim_burn_proof_of_knowledge
                        .ok_or_else(|| anyhow!("claim_burn_proof_of_knowledge not provided"))?
                        .try_into()
                        .map_err(|e| anyhow!("claim_burn_proof_of_knowledge: {}", e))?,
                    withdraw_proof: request.claim_burn_withdraw_proof.map(TryInto::try_into).transpose()?,
                }),
            },
            InstructionType::ClaimValidatorFees => Instruction::ClaimValidatorFees {
                address: request
                    .claim_validator_fees_address
                    .as_slice()
                    .try_into()
                    .map_err(|e| anyhow!("claim_validator_fees_address: {e}"))?,
            },
            InstructionType::DropAllProofsInWorkspace => Instruction::DropAllProofsInWorkspace,
            InstructionType::AssertBucketContains => {
                let resource_address = ObjectKey::try_from(request.resource_address)?.into();
                Instruction::AssertBucketContains {
                    key: request
                        .assert_bucket_workspace_id
                        .ok_or_else(|| anyhow!("assert_bucket_workspace_id not provided"))?
                        .try_into()?,
                    resource_address,
                    min_amount: request.min_amount.unwrap_or_default().into(),
                }
            },
            InstructionType::PublishTemplate => Instruction::PublishTemplate {
                binary: request.template_binary,
            },
            InstructionType::AllocateAddress => Instruction::AllocateAddress {
                allocatable_type: substate_type.try_into()?,
                workspace_id: WorkspaceId::try_from(request.allocate_address_workspace_id)
                    .context("allocate_address_workspace_id overflowed")?,
            },
            InstructionType::StealthTransfer => Instruction::StealthTransfer {
                resource_address_ref: request
                    .stealth_transfer_resource_address
                    .map(TryInto::try_into)
                    .transpose()?
                    .ok_or_else(|| anyhow!("stealth_transfer_resource_address not provided"))?,
                statement: request
                    .stealth_transfer_statement
                    .ok_or_else(|| anyhow!("stealth_transfer_statement not provided"))?
                    .try_into()
                    .context("stealth_transfer_statement conversion failed")?,
                revealed_input_bucket: request
                    .stealth_transfer_revealed_input_bucket
                    .map(TryInto::try_into)
                    .transpose()?,
            },
        };

        Ok(instruction)
    }
}

impl From<Instruction> for proto::transaction::Instruction {
    fn from(instruction: Instruction) -> Self {
        let mut result = proto::transaction::Instruction::default();

        match instruction {
            Instruction::CreateAccount {
                public_key_address,
                owner_rule,
                access_rules,
                workspace_id,
            } => {
                result.instruction_type = InstructionType::CreateAccount as i32;
                result.create_account_public_key = public_key_address.to_vec();
                result.create_account_owner_rule = owner_rule.map(Into::into);
                result.create_account_access_rules = access_rules.map(Into::into);
                result.create_account_workspace_id = workspace_id.map(Into::into);
            },
            Instruction::CallFunction {
                address: template_address,
                function,
                args,
            } => {
                result.instruction_type = InstructionType::Function as i32;
                result.template_address = template_address.to_vec();
                result.function = function;
                result.args = args.into_iter().map(|a| a.into()).collect();
            },
            Instruction::CallMethod { call, method, args } => {
                result.instruction_type = InstructionType::Method as i32;
                result.component_call = Some(call.into());
                result.method = method;
                result.args = args.into_iter().map(|a| a.into()).collect();
            },
            Instruction::PutLastInstructionOutputOnWorkspace { key } => {
                result.instruction_type = InstructionType::PutOutputInWorkspace as i32;
                result.workspace_put_key = u32::from(key);
            },
            Instruction::EmitLog { level, message } => {
                result.instruction_type = InstructionType::EmitLog as i32;
                result.log_level = level.to_string();
                result.log_message = message;
            },
            Instruction::ClaimBurn { claim } => {
                result.instruction_type = InstructionType::ClaimBurn as i32;
                result.claim_burn_commitment_address = claim.output_address.as_bytes().to_vec();
                result.claim_burn_range_proof = claim.range_proof.into_vec();
                result.claim_burn_proof_of_knowledge = Some(claim.proof_of_knowledge.into());
                result.claim_burn_public_key = claim.public_key.to_vec();
                result.claim_burn_withdraw_proof = claim.withdraw_proof.map(Into::into);
            },
            Instruction::ClaimValidatorFees { address } => {
                result.instruction_type = InstructionType::ClaimValidatorFees as i32;
                result.claim_validator_fees_address = address.as_slice().to_vec();
            },
            Instruction::DropAllProofsInWorkspace => {
                result.instruction_type = InstructionType::DropAllProofsInWorkspace as i32;
            },
            Instruction::AssertBucketContains {
                key,
                resource_address,
                min_amount,
            } => {
                result.instruction_type = InstructionType::AssertBucketContains as i32;
                result.assert_bucket_workspace_id = Some(key.into());
                result.resource_address = resource_address.as_bytes().to_vec();
                result.min_amount = Some(min_amount.into());
            },
            Instruction::PublishTemplate { binary } => {
                result.instruction_type = InstructionType::PublishTemplate as i32;
                result.template_binary = binary;
            },
            Instruction::AllocateAddress {
                allocatable_type,
                workspace_id,
            } => {
                result.instruction_type = InstructionType::AllocateAddress as i32;
                let substate_type: proto::transaction::AllocatableAddressType = allocatable_type.into();
                result.allocatable_address_type = substate_type as i32;
                result.allocate_address_workspace_id = u32::from(workspace_id);
            },
            Instruction::StealthTransfer {
                resource_address_ref: resource_address,
                statement,
                revealed_input_bucket,
            } => {
                result.instruction_type = InstructionType::StealthTransfer as i32;
                result.stealth_transfer_resource_address = Some(resource_address.into());
                result.stealth_transfer_statement = Some(statement.into());
                result.stealth_transfer_revealed_input_bucket = revealed_input_bucket.map(Into::into);
            },
        }
        result
    }
}

// -------------------------------- Arg -------------------------------- //

impl TryFrom<proto::transaction::Arg> for InstructionArg {
    type Error = anyhow::Error;

    fn try_from(request: proto::transaction::Arg) -> Result<Self, Self::Error> {
        let arg_value = request.arg_value.ok_or_else(|| anyhow!("arg_value not provided"))?;
        match arg_value {
            proto::transaction::arg::ArgValue::Literal(data) => Ok(InstructionArg::Literal(data)),
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
                arg_value: Some(proto::transaction::arg::ArgValue::Literal(data)),
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

// -------------------------------- CommitmentSignature -------------------------------- //

impl TryFrom<proto::transaction::CommitmentSignature> for CommitmentSignatureBytes {
    type Error = anyhow::Error;

    fn try_from(val: proto::transaction::CommitmentSignature) -> Result<Self, Self::Error> {
        let u = Scalar32Bytes::from_bytes(&val.signature_u).context("Invalid u signature")?;
        let v = Scalar32Bytes::from_bytes(&val.signature_v).context("Invalid v signature")?;
        let public_nonce =
            PedersenCommitmentBytes::from_bytes(&val.public_nonce_commitment).context("Invalid public nonce")?;

        Ok(Self::new(public_nonce, u, v))
    }
}

impl From<CommitmentSignatureBytes> for proto::transaction::CommitmentSignature {
    fn from(val: CommitmentSignatureBytes) -> Self {
        Self {
            public_nonce_commitment: val.public_nonce().to_vec(),
            signature_u: val.u().to_vec(),
            signature_v: val.v().to_vec(),
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
                .map_err(|e| anyhow!("Invalid balance proof signature: {}", e.to_error_string()))?,
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
            .map(|v| {
                RistrettoPublicKeyBytes::from_bytes(&v)
                    .map_err(|e| anyhow!("Invalid sender_public_nonce: {}", e.to_error_string()))
            })
            .transpose()?
            .ok_or_else(|| anyhow!("sender_public_nonce is missing"))?;

        Ok(UnspentOutput {
            commitment: checked_copy_fixed(&val.commitment)
                .ok_or_else(|| anyhow!("Invalid length of commitment bytes"))?,
            sender_public_nonce,
            encrypted_data: EncryptedData::try_from(val.encrypted_value)
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
            encrypted_value: val.encrypted_data.as_ref().to_vec(),
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
            .map_err(|e| anyhow!("Invalid owner public key: {}", e.to_error_string()))?;

        Ok(tari_template_lib::models::StealthUnspentOutput {
            output,
            owner_public_key,
        })
    }
}

impl From<tari_template_lib::models::StealthUnspentOutput> for proto::transaction::StealthUnspentOutput {
    fn from(val: tari_template_lib::models::StealthUnspentOutput) -> Self {
        Self {
            output: Some(val.output.into()),
            owner_public_key: val.owner_public_key.as_bytes().to_vec(),
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
            balance_proof: BalanceProofSignature::from_bytes(&value.balance_proof)
                .map_err(|e| anyhow!("Invalid balance proof signature: {}", e.to_error_string()))?,
        })
    }
}

impl From<tari_template_lib::models::StealthTransferStatement> for proto::transaction::StealthTransferStatement {
    fn from(value: tari_template_lib::models::StealthTransferStatement) -> Self {
        Self {
            inputs_statement: Some(value.inputs_statement.into()),
            outputs_statement: Some(value.outputs_statement.into()),
            balance_proof: value.balance_proof.to_bytes(),
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
        })
    }
}

impl From<tari_template_lib::models::StealthInputsStatement> for proto::transaction::StealthInputsStatement {
    fn from(value: tari_template_lib::models::StealthInputsStatement) -> Self {
        Self {
            inputs: value.inputs.iter().map(Into::into).collect(),
            revealed_amount: Some(value.revealed_amount.into()),
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
