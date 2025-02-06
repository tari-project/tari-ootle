//  Copyright 2022 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};
use tari_common_types::types::PublicKey;
use tari_template_lib::{
    args::{Arg, LogLevel},
    auth::OwnerRule,
    models::{ComponentAddress, ResourceAddress, TemplateAddress},
    prelude::{AccessRules, Amount},
};
#[cfg(feature = "ts")]
use ts_rs::TS;

use crate::{confidential::ConfidentialClaim, serde_with, vn_fee_pool::ValidatorFeePoolAddress};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(TS), ts(export, export_to = "../../bindings/src/types/"))]
pub enum Instruction {
    CreateAccount {
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        public_key_address: PublicKey,
        owner_rule: Option<OwnerRule>,
        access_rules: Option<AccessRules>,
        #[cfg_attr(feature = "ts", ts(type = "string | null"))]
        workspace_bucket: Option<String>,
    },
    CallFunction {
        #[serde(with = "serde_with::hex")]
        #[cfg_attr(feature = "ts", ts(type = "Uint8Array"))]
        template_address: TemplateAddress,
        function: String,
        #[serde(deserialize_with = "crate::argument_parser::json_deserialize")]
        args: Vec<Arg>,
    },
    CallMethod {
        component_address: ComponentAddress,
        method: String,
        #[serde(deserialize_with = "crate::argument_parser::json_deserialize")]
        // Argument parser takes an array of strings as input
        #[cfg_attr(feature = "ts", ts(type = "Array<string>"))]
        args: Vec<Arg>,
    },
    PutLastInstructionOutputOnWorkspace {
        key: Vec<u8>,
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
        key: Vec<u8>,
        resource_address: ResourceAddress,
        min_amount: Amount,
    },
    PublishTemplate {
        binary: Vec<u8>,
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
        if let Self::CallFunction { template_address, .. } = self {
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
                workspace_bucket,
            } => {
                write!(
                    f,
                    "CreateAccount {{ public_key_address: {}, owner_rule: {:?}, acces_rules: {:?}, bucket: ",
                    public_key_address, owner_rule, access_rules
                )?;
                match workspace_bucket {
                    Some(bucket) => write!(f, "{}", bucket)?,
                    None => write!(f, "None")?,
                }
                write!(f, " }}")
            },
            Self::CallFunction {
                template_address,
                function,
                args,
            } => write!(
                f,
                "CallFunction {{ template_address: {}, function: {}, args: {:?} }}",
                template_address, function, args
            ),
            Self::CallMethod {
                component_address,
                method,
                args,
            } => write!(
                f,
                "CallMethod {{ component_address: {}, method: {}, args: {:?} }}",
                component_address, method, args
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
                    claim.proof_of_knowledge.public_nonce().as_public_key(),
                    claim.proof_of_knowledge.u().reveal(),
                    claim.proof_of_knowledge.v().reveal(),
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
        }
    }
}
