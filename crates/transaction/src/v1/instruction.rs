//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::{Display, Formatter};

use tari_engine_types::confidential::MinotariBurnClaimProof;
use tari_ootle_common_types::displayable::Displayable;
use tari_template_lib_types::{
    Amount,
    FunctionName,
    OwnerRule,
    TemplateAddress,
    TemplateMetadataHash,
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
    MAX_INSTRUCTION_ARGS,
    ResourceAddressRef,
    args::{InstructionArg, WorkspaceId, WorkspaceOffsetId},
};

#[derive(Debug, Clone, PartialEq, borsh::BorshSerialize, minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Instruction {
    #[n(0)]
    CreateAccount {
        #[n(0)]
        owner_public_key: RistrettoPublicKeyBytes,
        #[n(1)]
        owner_rule: Option<OwnerRule>,
        #[n(2)]
        access_rules: Option<ComponentAccessRules>,
        #[n(3)]
        bucket_workspace_id: Option<WorkspaceOffsetId>,
    },
    #[n(1)]
    CallFunction {
        #[n(0)]
        address: TemplateAddress,
        #[n(1)]
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        function: FunctionName,
        #[n(2)]
        #[cbor(decode_with = "decode_args")]
        args: Vec<InstructionArg>,
    },
    #[n(2)]
    CallMethod {
        #[n(0)]
        call: ComponentReference,
        #[n(1)]
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        method: FunctionName,
        #[n(2)]
        #[cbor(decode_with = "decode_args")]
        args: Vec<InstructionArg>,
    },
    #[n(3)]
    PutLastInstructionOutputOnWorkspace {
        #[n(0)]
        key: WorkspaceId,
    },
    #[n(5)]
    ClaimBurn {
        #[n(0)]
        claim: Box<MinotariBurnClaimProof>,
    },
    #[n(6)]
    ClaimValidatorFees {
        #[n(0)]
        address: ValidatorFeePoolAddress,
        #[n(1)]
        max_amount: Option<Amount>,
    },
    #[n(7)]
    DropAllProofsInWorkspace,
    #[n(8)]
    Assert {
        #[n(0)]
        key: WorkspaceOffsetId,
        #[n(1)]
        assertion: Assertion,
    },
    #[n(9)]
    TakeFromBucket {
        #[n(0)]
        input_bucket: WorkspaceOffsetId,
        #[n(1)]
        amount: Amount,
        #[n(2)]
        output_bucket: WorkspaceId,
    },
    #[n(10)]
    PublishTemplate {
        /// Index into the transaction's `blobs` list. The referenced blob's bytes are the WASM
        /// binary, which the engine resolves via the surrounding `Blobs` at execution time.
        #[n(0)]
        binary: BlobIndex,
        /// Optional reference to off-chain template metadata, stored on the published template without
        /// interpretation (conventionally a multihash of the CBOR metadata)
        #[n(1)]
        #[cfg_attr(feature = "ts", ts(type = "string | null"))]
        metadata_hash: Option<TemplateMetadataHash>,
    },
    #[n(11)]
    AllocateAddress {
        #[n(0)]
        allocatable_type: AllocatableAddressType,
        #[n(1)]
        workspace_id: WorkspaceId,
    },
    #[n(12)]
    StealthTransfer {
        #[n(0)]
        resource_address_ref: ResourceAddressRef,
        #[n(1)]
        statement: StealthTransferStatement,
        #[n(2)]
        revealed_input_bucket: Option<WorkspaceOffsetId>,
    },
    #[n(13)]
    PayFeeFromBucket {
        #[n(0)]
        bucket: WorkspaceOffsetId,
    },
    #[n(14)]
    UpdateComponentTemplate {
        #[n(0)]
        component: ComponentReference,
        #[n(1)]
        migrate: Option<MigrateFunction>,
        #[n(2)]
        new_template: TemplateAddress,
    },
    /// Drains the bucket at `src` into the existing bucket at `dest`. The `src` workspace slot
    /// remains bound but its bucket is consumed; reusing it for further bucket operations will
    /// error at the bucket-state layer (matching `TakeFromBucket`'s slot semantics).
    #[n(15)]
    PutIntoBucket {
        #[n(0)]
        src: WorkspaceOffsetId,
        #[n(1)]
        dest: WorkspaceOffsetId,
    },
    /// Opens an authorization window over the workspace proof at `proof`: native instructions and calls that follow
    /// see it until a matching `DropProofAuthorization` or the end of the instruction phase.
    #[n(16)]
    AuthorizeProof {
        #[n(0)]
        proof: WorkspaceOffsetId,
    },
    /// Closes the authorization window over the workspace proof at `proof`. The proof stays on the workspace.
    #[n(17)]
    DropProofAuthorization {
        #[n(0)]
        proof: WorkspaceOffsetId,
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

    fn referenced_template(&self) -> Option<&TemplateAddress> {
        match self {
            Self::CallFunction { address, .. } => Some(address),
            // `UpdateComponentTemplate` loads the template replacing the component's, and never the one it replaces.
            Self::UpdateComponentTemplate { new_template, .. } => Some(new_template),
            _ => None,
        }
    }

    pub fn referenced_templates(&self) -> impl Iterator<Item = &TemplateAddress> + '_ {
        let single = self.referenced_template();
        let stealth = match self {
            Self::StealthTransfer { statement, .. } => {
                Some(statement.script_path_template_functions().map(|tf| &tf.template))
            },
            _ => None,
        };
        single.into_iter().chain(stealth.into_iter().flatten())
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
            Self::ClaimBurn { .. } |
            Self::ClaimValidatorFees { .. } |
            Self::DropAllProofsInWorkspace |
            Self::Assert { .. } |
            Self::TakeFromBucket { .. } |
            Self::PutIntoBucket { .. } |
            Self::AllocateAddress { .. } |
            Self::StealthTransfer { .. } |
            Self::PayFeeFromBucket { .. } |
            Self::AuthorizeProof { .. } |
            Self::DropProofAuthorization { .. } => {},
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
            Self::ClaimBurn { .. } |
            Self::ClaimValidatorFees { .. } |
            Self::DropAllProofsInWorkspace |
            Self::Assert { .. } |
            Self::TakeFromBucket { .. } |
            Self::PutIntoBucket { .. } |
            Self::AllocateAddress { .. } |
            Self::StealthTransfer { .. } |
            Self::PayFeeFromBucket { .. } |
            Self::AuthorizeProof { .. } |
            Self::DropProofAuthorization { .. } => {},
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

    /// Whether this instruction can make a fee payment: `PayFeeFromBucket` directly, or a call into template code,
    /// which may pay from a vault. Whether the fee intent pays enough is settled at the fee checkpoint.
    pub fn may_pay_fee(&self) -> bool {
        match self {
            Self::PayFeeFromBucket { .. } | Self::CallMethod { .. } | Self::CallFunction { .. } => true,
            Self::CreateAccount { .. } |
            Self::PutLastInstructionOutputOnWorkspace { .. } |
            Self::ClaimBurn { .. } |
            Self::ClaimValidatorFees { .. } |
            Self::DropAllProofsInWorkspace |
            Self::Assert { .. } |
            Self::TakeFromBucket { .. } |
            Self::PublishTemplate { .. } |
            Self::AllocateAddress { .. } |
            Self::StealthTransfer { .. } |
            Self::UpdateComponentTemplate { .. } |
            Self::PutIntoBucket { .. } |
            Self::AuthorizeProof { .. } |
            Self::DropProofAuthorization { .. } => false,
        }
    }

    /// Whether this instruction is permitted inside a transaction's fee instructions.
    ///
    /// The fee intent runs on a flat compute credit and its writes commit even when the main intent fails, so it
    /// holds only the work of funding the fee. Publishing a template and changing a component's template are main
    /// intent work, funded by the fee already paid.
    pub fn is_allowed_in_fee_intent(&self) -> bool {
        match self {
            Self::PublishTemplate { .. } | Self::UpdateComponentTemplate { .. } => false,
            Self::CreateAccount { .. } |
            Self::CallFunction { .. } |
            Self::CallMethod { .. } |
            Self::PutLastInstructionOutputOnWorkspace { .. } |
            Self::ClaimBurn { .. } |
            Self::ClaimValidatorFees { .. } |
            Self::DropAllProofsInWorkspace |
            Self::Assert { .. } |
            Self::TakeFromBucket { .. } |
            Self::AllocateAddress { .. } |
            Self::StealthTransfer { .. } |
            Self::PayFeeFromBucket { .. } |
            Self::PutIntoBucket { .. } |
            Self::AuthorizeProof { .. } |
            Self::DropProofAuthorization { .. } => true,
        }
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
            Self::PutIntoBucket { src, dest } => {
                src.remap_id(id_offset);
                dest.remap_id(id_offset);
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
            Self::AuthorizeProof { proof } | Self::DropProofAuthorization { proof } => {
                proof.remap_id(id_offset);
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
            Self::ClaimBurn { claim, .. } => {
                write!(f, "ClaimBurn {{ {claim} }}",)
            },
            Self::ClaimValidatorFees { address, max_amount } => {
                if let Some(max_amount) = max_amount {
                    write!(
                        f,
                        "ClaimValidatorFees {{ address: {address}, max_amount: {max_amount} }}"
                    )
                } else {
                    write!(f, "ClaimValidatorFees {{ address: {address} }}")
                }
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
            Self::PutIntoBucket { src, dest } => {
                write!(f, "PutIntoBucket {{ src: {}, dest: {} }}", src, dest)
            },
            Self::AuthorizeProof { proof } => {
                write!(f, "AuthorizeProof {{ proof: {} }}", proof)
            },
            Self::DropProofAuthorization { proof } => {
                write!(f, "DropProofAuthorization {{ proof: {} }}", proof)
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

#[derive(Debug, Clone, PartialEq, borsh::BorshSerialize, minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct MigrateFunction {
    #[n(0)]
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub name: FunctionName,
    #[n(1)]
    #[cbor(decode_with = "decode_args")]
    pub args: Vec<InstructionArg>,
}

fn decode_args<'b, C>(
    d: &mut minicbor::Decoder<'b>,
    ctx: &mut C,
) -> Result<Vec<InstructionArg>, minicbor::decode::Error> {
    tari_bor::adapters::bounded_vec::decode(d, ctx, MAX_INSTRUCTION_ARGS)
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

    #[cfg(feature = "serde")]
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
    fn decode_encode_minicbor() {
        let instruction = make_sample();
        let encoded = tari_bor::encode(&instruction).unwrap();
        let decoded: Instruction = tari_bor::decode(&encoded).unwrap();
        assert_eq!(instruction, decoded);

        let instruction = Instruction::PutIntoBucket {
            src: WorkspaceOffsetId::new(1),
            dest: WorkspaceOffsetId::new(2),
        };
        let encoded = tari_bor::encode(&instruction).unwrap();
        let decoded: Instruction = tari_bor::decode(&encoded).unwrap();
        assert_eq!(instruction, decoded);

        for instruction in [
            Instruction::AuthorizeProof {
                proof: WorkspaceOffsetId::new(3).with_offset(1),
            },
            Instruction::DropProofAuthorization {
                proof: WorkspaceOffsetId::new(3),
            },
        ] {
            let encoded = tari_bor::encode(&instruction).unwrap();
            let decoded: Instruction = tari_bor::decode(&encoded).unwrap();
            assert_eq!(instruction, decoded);
        }
    }

    /// Pins the borsh bytes of the authorization window instructions in the signing preimage: the variant's
    /// declaration index (15 and 16, distinct from their CBOR tags 16 and 17), then the workspace id as a
    /// little-endian `u16` and the optional offset as a little-endian `u64`.
    #[test]
    fn proof_authorization_borsh_preimage() {
        let instruction = Instruction::AuthorizeProof {
            proof: WorkspaceOffsetId::new(3),
        };
        assert_eq!(borsh::to_vec(&instruction).unwrap(), [15, 3, 0, 0]);

        let instruction = Instruction::AuthorizeProof {
            proof: WorkspaceOffsetId::new(3).with_offset(1),
        };
        assert_eq!(borsh::to_vec(&instruction).unwrap(), [
            15, 3, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0
        ]);

        let instruction = Instruction::DropProofAuthorization {
            proof: WorkspaceOffsetId::new(3),
        };
        assert_eq!(borsh::to_vec(&instruction).unwrap(), [16, 3, 0, 0]);
    }

    #[test]
    fn remap_workspace_ids_shifts_proof_authorization() {
        let mut instruction = Instruction::AuthorizeProof {
            proof: WorkspaceOffsetId::new(3).with_offset(1),
        };
        instruction.remap_workspace_ids(10);
        assert_eq!(instruction, Instruction::AuthorizeProof {
            proof: WorkspaceOffsetId::new(13).with_offset(1),
        });

        let mut instruction = Instruction::DropProofAuthorization {
            proof: WorkspaceOffsetId::new(3),
        };
        instruction.remap_workspace_ids(10);
        assert_eq!(instruction, Instruction::DropProofAuthorization {
            proof: WorkspaceOffsetId::new(13),
        });
    }

    /// Pins the borsh bytes that a publish instruction contributes to the signing preimage: the variant's declaration
    /// index (9, distinct from its CBOR tag 10), the blob index, the `Some` tag, then the metadata hash as a
    /// little-endian `u32` length and its bytes. Hardware and host-SDK signers reproduce this byte for byte.
    #[test]
    fn publish_template_borsh_preimage() {
        let instruction = Instruction::PublishTemplate {
            binary: 7,
            metadata_hash: TemplateMetadataHash::from_bytes(vec![0xaa, 0xbb, 0xcc]),
        };
        assert_eq!(borsh::to_vec(&instruction).unwrap(), [
            9, 7, 1, 3, 0, 0, 0, 0xaa, 0xbb, 0xcc
        ]);

        let instruction = Instruction::PublishTemplate {
            binary: 7,
            metadata_hash: None,
        };
        assert_eq!(borsh::to_vec(&instruction).unwrap(), [9, 7, 0]);
    }
}
