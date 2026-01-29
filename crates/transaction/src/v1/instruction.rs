//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};
use tari_engine_types::{
    confidential::{ClaimBurnOutputData, MinotariBurnClaimProof},
    limits,
    published_template::TemplateBlob,
};
use tari_ootle_common_types::displayable::Displayable;
use tari_template_lib::{
    args::LogLevel,
    types::{
        access_rules::ComponentAccessRules,
        crypto::RistrettoPublicKeyBytes,
        stealth::StealthTransferStatement,
        Amount,
        FunctionName,
        MaxString,
        OwnerRule,
        ResourceAddress,
        TemplateAddress,
        ValidatorFeePoolAddress,
    },
};

use crate::{
    args::{InstructionArg, WorkspaceId, WorkspaceOffsetId},
    AllocatableAddressType,
    ComponentReference,
    ResourceAddressRef,
};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Instruction {
    CreateAccount {
        owner_public_key: RistrettoPublicKeyBytes,
        owner_rule: Option<OwnerRule>,
        access_rules: Option<ComponentAccessRules>,
        bucket_workspace_id: Option<WorkspaceOffsetId>,
    },
    CallFunction {
        address: TemplateAddress,
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        function: FunctionName,
        // #[serde(deserialize_with = "crate::special_json_arg_syntax::json_deserialize")]
        // #[cfg_attr(feature = "ts", ts(type = "Array<any>"))]
        args: Vec<InstructionArg>,
    },
    CallMethod {
        call: ComponentReference,
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        method: FunctionName,
        // TODO: remove this as it causes tricky issues that are hard to track down (typically Signature errors).
        // Rather have clients provide raw arguments using CBOR.
        // #[serde(deserialize_with = "crate::special_json_arg_syntax::json_deserialize")]
        // Argument parser takes an array of strings as input
        // #[cfg_attr(feature = "ts", ts(type = "Array<any>"))]
        args: Vec<InstructionArg>,
    },
    PutLastInstructionOutputOnWorkspace {
        key: WorkspaceId,
    },
    EmitLog {
        level: LogLevel,
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        message: MaxString<{ limits::ENGINE_LIMITS.max_log_size_bytes }>,
    },
    ClaimBurn {
        claim: Box<MinotariBurnClaimProof>,
        output_data: ClaimBurnOutputData,
    },
    ClaimValidatorFees {
        address: ValidatorFeePoolAddress,
    },
    DropAllProofsInWorkspace,
    AssertBucketContains {
        key: WorkspaceOffsetId,
        resource_address: ResourceAddress,
        min_amount: Amount,
    },
    TakeFromBucket {
        input_bucket: WorkspaceOffsetId,
        amount: Amount,
        output_bucket: WorkspaceId,
    },
    PublishTemplate {
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        #[serde(with = "ootle_serde::base64")]
        binary: TemplateBlob,
    },
    AllocateAddress {
        allocatable_type: AllocatableAddressType,
        workspace_id: WorkspaceId,
    },
    StealthTransfer {
        resource_address_ref: ResourceAddressRef,
        statement: StealthTransferStatement,
        revealed_input_bucket: Option<WorkspaceOffsetId>,
    },
    PayFeeStealth {
        statement: StealthTransferStatement,
        revealed_input_bucket: Option<WorkspaceOffsetId>,
    },
    PayFeeFromBucket {
        bucket: WorkspaceOffsetId,
    },
    UpdateComponentTemplate {
        component: ComponentReference,
        migrate: Option<MigrateFunction>,
        new_template: TemplateAddress,
    },
}

impl Instruction {
    pub fn published_template_binary(&self) -> Option<&[u8]> {
        match self {
            Self::PublishTemplate { binary } => Some(binary),
            _ => None,
        }
    }

    pub fn referenced_template(&self) -> Option<&TemplateAddress> {
        match self {
            Self::CallFunction { address, .. } => Some(address),
            _ => None,
        }
    }

    pub fn claim_burn(&self) -> Option<&MinotariBurnClaimProof> {
        match self {
            Self::ClaimBurn { claim, .. } => Some(claim),
            _ => None,
        }
    }

    pub fn is_pay_fee(&self) -> bool {
        matches!(self, Self::PayFeeStealth { .. })
    }

    pub fn allocated_workspace_id(&self) -> Option<WorkspaceId> {
        // These instructions allocate addresses a workspace ID
        match self {
            Self::PutLastInstructionOutputOnWorkspace { key } => Some(*key),
            Self::TakeFromBucket { output_bucket, .. } => Some(*output_bucket),
            Self::AllocateAddress { workspace_id, .. } => Some(*workspace_id),
            _ => None,
        }
    }
}

impl Display for Instruction {
    #[allow(clippy::too_many_lines)]
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateAccount {
                owner_public_key: public_key_address,
                owner_rule,
                access_rules,
                bucket_workspace_id: workspace_id,
            } => {
                write!(
                    f,
                    "CreateAccount {{ public_key_address: {}, owner_rule: {:?}, access_rules: {:?}, bucket: ",
                    public_key_address, owner_rule, access_rules
                )?;
                match workspace_id {
                    Some(id) => write!(f, "Some({})", id)?,
                    None => write!(f, "None")?,
                }
                write!(f, " }}")
            },
            Self::CallFunction {
                address,
                function,
                args,
            } => {
                write!(
                    f,
                    "CallFunction {{ address: {}, function: {}, num args: {} }}",
                    address,
                    function,
                    args.len()
                )
            },
            Self::CallMethod { call, method, args } => write!(
                f,
                "CallMethod {{ call: {}, method: {}, num args: {} }}",
                call,
                method,
                args.len()
            ),
            Self::PutLastInstructionOutputOnWorkspace { key } => {
                write!(f, "PutLastInstructionOutputOnWorkspace {{ key: {key} }}")
            },
            Self::EmitLog { level, message } => {
                write!(f, "EmitLog {{ level: {level}, message: {message} }}")
            },
            Self::ClaimBurn { claim, .. } => {
                write!(f, "ClaimBurn {{ {claim} }}",)
            },
            Self::ClaimValidatorFees { address } => {
                write!(f, "ClaimValidatorFees {{ address: {} }}", address)
            },

            Self::DropAllProofsInWorkspace => {
                write!(f, "DropAllProofsInWorkspace")
            },
            Self::AssertBucketContains {
                key,
                resource_address,
                min_amount,
            } => {
                write!(
                    f,
                    "AssertBucketContains {{ key: {:?}, resource_address: {}, min_amount: {} }}",
                    key, resource_address, min_amount
                )
            },

            Self::TakeFromBucket {
                input_bucket,
                amount,
                output_bucket,
            } => {
                write!(
                    f,
                    "TakeFromBucket {{ input_bucket: {}, amount: {}, output_bucket: {} }}",
                    input_bucket, amount, output_bucket
                )
            },
            Self::PublishTemplate { .. } => {
                write!(f, "PublishTemplate")
            },
            Self::AllocateAddress {
                allocatable_type: substate_type,
                workspace_id,
            } => {
                write!(
                    f,
                    "AllocateAddress {{ substate_type: {substate_type:?}, workspace ID: {workspace_id} }}"
                )
            },
            Self::StealthTransfer {
                resource_address_ref: resource_address,
                statement,
                revealed_input_bucket: bucket,
            } => {
                write!(
                    f,
                    "StealthTransfer {{ resource_address: {}, output(s): {}, rp-size: {}",
                    resource_address,
                    statement.outputs_statement.outputs.len(),
                    statement.outputs_statement.agg_range_proof.len(),
                )?;
                match bucket {
                    Some(id) => write!(f, ", revealed_input_bucket: Some({}) }}", id),
                    None => write!(f, ", revealed_input_bucket: None }}"),
                }
            },
            Self::PayFeeStealth {
                statement,
                revealed_input_bucket: bucket,
            } => {
                write!(
                    f,
                    "PayFee {{ revealed_input: {}, output(s): {}, rp-size: {}",
                    statement.inputs_statement.revealed_amount,
                    statement.outputs_statement.outputs.len(),
                    statement.outputs_statement.agg_range_proof.len(),
                )?;
                match bucket {
                    Some(id) => write!(f, ", revealed_input_bucket: Some({}) }}", id),
                    None => write!(f, ", revealed_input_bucket: None }}"),
                }
            },
            Self::PayFeeFromBucket { bucket } => {
                write!(f, "PayFeeFromBucket {{ bucket: {} }}", bucket)
            },
            Self::UpdateComponentTemplate {
                component,
                migrate,
                new_template,
            } => {
                write!(
                    f,
                    "UpdateComponentTemplate {{ component: {}, migrate: {}, new_template: {} }}",
                    component,
                    migrate.display(),
                    new_template
                )
            },
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct MigrateFunction {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub name: FunctionName,
    pub args: Vec<InstructionArg>,
}

impl Display for MigrateFunction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Migrate {{ name: {}, num_args: {} }}", self.name, self.args.len())
    }
}

#[cfg(test)]
mod tests {
    use tari_template_lib::types::constants::XTR;

    use super::*;
    use crate::call_args;

    fn make_sample() -> Instruction {
        Instruction::CallFunction {
            address: Default::default(),
            function: "test".try_into().unwrap(),
            args: call_args![("A", "B"), 123u64, true, vec![1, 2, 3], XTR],
        }
    }

    #[test]
    fn decode_encode_json() {
        let instruction = make_sample();
        let json = serde_json::to_string(&instruction).unwrap();
        let decoded: Instruction = serde_json::from_str(&json).unwrap();
        assert_eq!(instruction, decoded);

        let instruction = Instruction::PublishTemplate {
            binary: vec![1, 2, 3].try_into().unwrap(),
        };
        let json = serde_json::to_string(&instruction).unwrap();
        let decoded: Instruction = serde_json::from_str(&json).unwrap();
        assert_eq!(instruction, decoded);
    }

    #[test]
    fn decode_encode_bincode() {
        let instruction = make_sample();
        let encoded = bincode::serde::encode_to_vec(&instruction, bincode::config::standard()).unwrap();
        let (decoded, _): (Instruction, usize) =
            bincode::serde::decode_from_slice(&encoded, bincode::config::standard()).unwrap();
        assert_eq!(instruction, decoded);
    }
}
