//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};
use tari_engine_types::{
    confidential::{ClaimBurnOutputData, MinotariBurnClaimProof},
    limits,
};
use tari_ootle_common_types::displayable::Displayable;
use tari_ootle_template_metadata::MetadataHash;
use tari_template_lib_types::{
    Amount,
    FunctionName,
    LogLevel,
    MaxString,
    OwnerRule,
    TemplateAddress,
    ValidatorFeePoolAddress,
    access_rules::ComponentAccessRules,
    crypto::RistrettoPublicKeyBytes,
    stealth::StealthTransferStatement,
};

use crate::{
    AllocatableAddressType,
    Assertion,
    BlobIndex,
    ComponentReference,
    ResourceAddressRef,
    args::{InstructionArg, WorkspaceId, WorkspaceOffsetId},
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
        args: Vec<InstructionArg>,
    },
    CallMethod {
        call: ComponentReference,
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        method: FunctionName,
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
    Assert {
        key: WorkspaceOffsetId,
        assertion: Assertion,
    },
    TakeFromBucket {
        input_bucket: WorkspaceOffsetId,
        amount: Amount,
        output_bucket: WorkspaceId,
    },
    PublishTemplate {
        /// Index into the transaction's `blobs` list. The referenced blob's bytes are the WASM
        /// binary, which the engine resolves via the surrounding `Blobs` at execution time.
        binary: BlobIndex,
        /// Optional multihash of off-chain CBOR metadata
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(type = "string | null"))]
        metadata_hash: Option<MetadataHash>,
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
    /// Returns the `BlobIndex` of the WASM binary for `PublishTemplate` instructions.
    pub fn published_template_binary_index(&self) -> Option<BlobIndex> {
        match self {
            Self::PublishTemplate { binary, .. } => Some(*binary),
            _ => None,
        }
    }

    pub fn referenced_template(&self) -> Option<&TemplateAddress> {
        match self {
            Self::CallFunction { address, .. } => Some(address),
            _ => None,
        }
    }

    /// Iterate over every `BlobIndex` this instruction references — both `PublishTemplate.binary`
    /// and `InstructionArg::Blob` argument references.
    ///
    /// NOTE: Every variant is listed explicitly (no wildcard `_` arm) so adding a new
    /// `Instruction` variant produces a compile error here, forcing the author to declare any
    /// blob references the variant introduces.
    pub fn referenced_blob_ids(&self) -> Vec<BlobIndex> {
        let mut out = Vec::new();
        match self {
            Self::PublishTemplate { binary, .. } => out.push(*binary),
            Self::CallFunction { args, .. } => collect_arg_blob_ids(args, &mut out),
            Self::CallMethod { args, .. } => collect_arg_blob_ids(args, &mut out),
            Self::UpdateComponentTemplate { migrate, .. } => {
                if let Some(m) = migrate {
                    collect_arg_blob_ids(&m.args, &mut out);
                }
            },
            // No blob references in these variants
            Self::CreateAccount { .. } |
            Self::PutLastInstructionOutputOnWorkspace { .. } |
            Self::EmitLog { .. } |
            Self::ClaimBurn { .. } |
            Self::ClaimValidatorFees { .. } |
            Self::DropAllProofsInWorkspace |
            Self::Assert { .. } |
            Self::TakeFromBucket { .. } |
            Self::AllocateAddress { .. } |
            Self::StealthTransfer { .. } |
            Self::PayFeeFromBucket { .. } => {},
        }
        out
    }

    /// Shift every `BlobIndex` in this instruction by the given offset. Used when merging two
    /// transaction builders to avoid blob-index collisions, mirroring `remap_workspace_ids`.
    ///
    /// Panics on overflow.
    pub fn remap_blob_ids(&mut self, id_offset: BlobIndex) {
        if id_offset == 0 {
            return;
        }
        match self {
            Self::PublishTemplate { binary, .. } => {
                *binary = binary.checked_add(id_offset).expect("BlobIndex overflow during merge");
            },
            Self::CallFunction { args, .. } => {
                for arg in args {
                    arg.remap_blob_id(id_offset);
                }
            },
            Self::CallMethod { args, .. } => {
                for arg in args {
                    arg.remap_blob_id(id_offset);
                }
            },
            Self::UpdateComponentTemplate { migrate, .. } => {
                if let Some(m) = migrate {
                    for arg in &mut m.args {
                        arg.remap_blob_id(id_offset);
                    }
                }
            },
            // No blob references in these variants
            Self::CreateAccount { .. } |
            Self::PutLastInstructionOutputOnWorkspace { .. } |
            Self::EmitLog { .. } |
            Self::ClaimBurn { .. } |
            Self::ClaimValidatorFees { .. } |
            Self::DropAllProofsInWorkspace |
            Self::Assert { .. } |
            Self::TakeFromBucket { .. } |
            Self::AllocateAddress { .. } |
            Self::StealthTransfer { .. } |
            Self::PayFeeFromBucket { .. } => {},
        }
    }

    pub fn claim_burn(&self) -> Option<&MinotariBurnClaimProof> {
        match self {
            Self::ClaimBurn { claim, .. } => Some(claim),
            _ => None,
        }
    }

    pub fn is_pay_fee(&self) -> bool {
        matches!(self, Self::PayFeeFromBucket { .. })
    }

    /// Shift all workspace IDs in this instruction by the given offset.
    /// Used when merging two transaction builders to avoid workspace ID collisions.
    ///
    /// NOTE: Every variant is listed explicitly (no wildcard `_` arm) so that adding a new
    /// `Instruction` variant produces a compile error here, forcing the author to handle
    /// workspace ID remapping.
    pub fn remap_workspace_ids(&mut self, id_offset: WorkspaceId) {
        if id_offset == 0 {
            return;
        }
        match self {
            Self::CreateAccount {
                bucket_workspace_id, ..
            } => {
                if let Some(id) = bucket_workspace_id {
                    id.remap_id(id_offset);
                }
            },
            Self::CallFunction { args, .. } => {
                for arg in args {
                    arg.remap_workspace_id(id_offset);
                }
            },
            Self::CallMethod { call, args, .. } => {
                call.remap_workspace_id(id_offset);
                for arg in args {
                    arg.remap_workspace_id(id_offset);
                }
            },
            Self::PutLastInstructionOutputOnWorkspace { key } => {
                *key = key.checked_add(id_offset).expect("Workspace ID overflow during merge");
            },
            Self::Assert { key, .. } => {
                key.remap_id(id_offset);
            },
            Self::TakeFromBucket {
                input_bucket,
                output_bucket,
                ..
            } => {
                input_bucket.remap_id(id_offset);
                *output_bucket = output_bucket
                    .checked_add(id_offset)
                    .expect("Workspace ID overflow during merge");
            },
            Self::AllocateAddress { workspace_id, .. } => {
                *workspace_id = workspace_id
                    .checked_add(id_offset)
                    .expect("Workspace ID overflow during merge");
            },
            Self::StealthTransfer {
                resource_address_ref,
                revealed_input_bucket,
                ..
            } => {
                resource_address_ref.remap_workspace_id(id_offset);
                if let Some(id) = revealed_input_bucket {
                    id.remap_id(id_offset);
                }
            },
            Self::PayFeeFromBucket { bucket } => {
                bucket.remap_id(id_offset);
            },
            Self::UpdateComponentTemplate { component, migrate, .. } => {
                component.remap_workspace_id(id_offset);
                if let Some(migrate) = migrate {
                    for arg in &mut migrate.args {
                        arg.remap_workspace_id(id_offset);
                    }
                }
            },
            // No workspace IDs in these variants
            Self::EmitLog { .. } |
            Self::ClaimBurn { .. } |
            Self::ClaimValidatorFees { .. } |
            Self::DropAllProofsInWorkspace |
            Self::PublishTemplate { .. } => {},
        }
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
            Self::Assert { key, assertion } => {
                write!(f, "Assert {{ key: {}, assertion: {} }}", key, assertion)
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

fn collect_arg_blob_ids(args: &[InstructionArg], out: &mut Vec<BlobIndex>) {
    for arg in args {
        if let Some(idx) = arg.as_blob_index() {
            out.push(idx);
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
    use tari_template_lib_types::constants::TARI_TOKEN;

    use super::*;
    use crate::call_args;

    fn make_sample() -> Instruction {
        Instruction::CallFunction {
            address: Default::default(),
            function: "test".try_into().unwrap(),
            args: call_args![("A", "B"), 123u64, true, vec![1, 2, 3], TARI_TOKEN],
        }
    }

    #[test]
    fn decode_encode_json() {
        let instruction = make_sample();
        let json = serde_json::to_string(&instruction).unwrap();
        let decoded: Instruction = serde_json::from_str(&json).unwrap();
        assert_eq!(instruction, decoded);

        let instruction = Instruction::PublishTemplate {
            binary: 0,
            metadata_hash: None,
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
