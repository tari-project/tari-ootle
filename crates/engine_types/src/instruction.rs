//  Copyright 2022 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};
use tari_template_lib::{
    args::{AllocatableAddressType, InstructionArg, LogLevel, WorkspaceId, WorkspaceOffsetId},
    auth::OwnerRule,
    models::ResourceAddress,
    prelude::{AccessRules, Amount},
    types::{crypto::RistrettoPublicKeyBytes, TemplateAddress},
};

use crate::{confidential::ConfidentialClaim, instruction_call::ComponentCall, ValidatorFeePoolAddress};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub enum Instruction {
    CreateAccount {
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        public_key_address: RistrettoPublicKeyBytes,
        owner_rule: Option<OwnerRule>,
        access_rules: Option<AccessRules>,
        workspace_id: Option<WorkspaceOffsetId>,
    },
    CallFunction {
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        address: TemplateAddress,
        function: String,
        #[serde(deserialize_with = "crate::argument_parser::json_deserialize")]
        #[cfg_attr(feature = "ts", ts(type = "Array<string | object>"))]
        args: Vec<InstructionArg>,
    },
    CallMethod {
        call: ComponentCall,
        method: String,
        #[serde(deserialize_with = "crate::argument_parser::json_deserialize")]
        // Argument parser takes an array of strings as input
        #[cfg_attr(feature = "ts", ts(type = "Array<string | object>"))]
        args: Vec<InstructionArg>,
    },
    PutLastInstructionOutputOnWorkspace {
        key: WorkspaceId,
    },
    EmitLog {
        level: LogLevel,
        message: String,
    },
    ClaimBurn {
        claim: Box<ConfidentialClaim>,
    },
    ClaimValidatorFees {
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        address: ValidatorFeePoolAddress,
    },
    DropAllProofsInWorkspace,
    AssertBucketContains {
        key: WorkspaceOffsetId,
        resource_address: ResourceAddress,
        min_amount: Amount,
    },
    PublishTemplate {
        binary: Vec<u8>,
    },
    AllocateAddress {
        allocatable_type: AllocatableAddressType,
        workspace_id: WorkspaceId,
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
        if let Self::CallFunction {
            address: template_address,
            ..
        } = self
        {
            return Some(template_address);
        }
        None
    }
}

impl Display for Instruction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateAccount {
                public_key_address,
                owner_rule,
                access_rules,
                workspace_id,
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
            } => write!(
                f,
                "CallFunction {{ address: {}, function: {}, args: {:?} }}",
                address, function, args
            ),
            Self::CallMethod { call, method, args } => write!(
                f,
                "CallMethod {{ call: {}, method: {}, args: {:?} }}",
                call, method, args
            ),
            Self::PutLastInstructionOutputOnWorkspace { key } => {
                write!(f, "PutLastInstructionOutputOnWorkspace {{ key: {:?} }}", key)
            },
            Self::EmitLog { level, message } => {
                write!(f, "EmitLog {{ level: {:?}, message: {:?} }}", level, message)
            },
            Self::ClaimBurn { claim } => {
                write!(
                    f,
                    "ClaimBurn {{ commitment_address: {}, proof_of_knowledge: nonce({}), u({}) v({}) }}",
                    claim.output_address,
                    claim.proof_of_knowledge.public_nonce(),
                    claim.proof_of_knowledge.u(),
                    claim.proof_of_knowledge.v(),
                )
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
            Instruction::PublishTemplate { .. } => {
                write!(f, "PublishTemplate")
            },
            Instruction::AllocateAddress {
                allocatable_type: substate_type,
                workspace_id,
            } => {
                write!(
                    f,
                    "AllocateAddress {{ substate_type: {substate_type:?}, workspace ID: {workspace_id} }}"
                )
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use tari_template_lib::call_args;

    use super::*;

    #[test]
    fn decode_encode() {
        let instruction = Instruction::CallFunction {
            address: Default::default(),
            function: "test".to_string(),
            args: call_args![("A", "B"), 123u64, true, vec![1, 2, 3]],
        };
        let json = serde_json::to_string(&instruction).unwrap();
        let decoded: Instruction = serde_json::from_str(&json).unwrap();
        assert_eq!(instruction, decoded);

        let instruction = Instruction::PublishTemplate { binary: vec![1, 2, 3] };
        let json = serde_json::to_string(&instruction).unwrap();
        let decoded: Instruction = serde_json::from_str(&json).unwrap();
        assert_eq!(instruction, decoded);
    }
}
