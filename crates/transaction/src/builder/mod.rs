//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub mod named_args;
mod named_component_call;
mod named_resource_ref;
#[cfg(test)]
mod tests;
mod workspace_ids;

pub use named_component_call::*;
use tari_crypto::ristretto::RistrettoSecretKey;
use tari_engine_types::{
    confidential::{ClaimBurnOutputData, MinotariBurnClaimProof},
    substate::SubstateId,
    ValidatorFeePoolAddress,
};
use tari_ootle_common_types::{Epoch, SubstateRequirement};
use tari_template_lib::{
    auth::OwnerRule,
    models::{ResourceAddress, StealthTransferStatement},
    prelude::AccessRules,
    types::{crypto::RistrettoPublicKeyBytes, Amount, TemplateAddress},
};

use crate::{
    args::{InstructionArg, WorkspaceOffsetId},
    builder::{
        named_args::{parse_workspace_key, BuilderWorkspaceKey, NamedArg, ParseWorkspaceKeyError},
        named_resource_ref::NamedResourceRef,
        workspace_ids::WorkspaceIds,
    },
    call_args,
    unsigned_transaction::UnsignedTransaction,
    AllocatableAddressType,
    ComponentCall,
    Instruction,
    ResourceAddressRef,
    Transaction,
    TransactionSignature,
    UnsealedTransactionV1,
};

#[derive(Debug, Clone, Default)]
pub struct TransactionBuilder {
    unsigned_transaction: UnsignedTransaction,
    signatures: Vec<TransactionSignature>,
    workspace_ids: WorkspaceIds,
}

impl TransactionBuilder {
    pub fn new() -> Self {
        Self {
            unsigned_transaction: UnsignedTransaction::default(),
            signatures: vec![],
            workspace_ids: WorkspaceIds::new(),
        }
    }

    pub fn with_unsigned_transaction<T: Into<UnsignedTransaction>>(self, unsigned_transaction: T) -> Self {
        Self {
            unsigned_transaction: unsigned_transaction.into(),
            signatures: vec![],
            workspace_ids: WorkspaceIds::new(),
        }
    }

    pub fn then<F: FnOnce(Self) -> Self>(self, f: F) -> Self {
        f(self)
    }

    pub fn for_network<N: Into<u8>>(mut self, network: N) -> Self {
        self.unsigned_transaction.set_network(network);
        self
    }

    pub fn with_dry_run(mut self, dry_run: bool) -> Self {
        self.unsigned_transaction.set_dry_run(dry_run);
        self
    }

    pub fn with_authorized_seal_signer(mut self) -> Self {
        self.unsigned_transaction = self.unsigned_transaction.authorized_sealed_signer();
        self.clear_signatures();
        self
    }

    /// Pays fees using a stealth transfer statement. The statement must reveal sufficient funds to cover the fee.
    /// NOTE: fees paid are not refunded, so any overpayment is kept by validators.
    pub fn fee_transaction_pay_fees_stealth(self, statement: StealthTransferStatement) -> Self {
        self.with_fee_instructions_builder(|builder| builder.pay_fee_stealth(statement))
    }

    /// Adds a fee instruction that calls the "take_fee" method on a component.
    /// This method must exist and return a Bucket with containing revealed confidential XTR resource.
    /// This allows the fee to originate from sources other than the transaction sender's account.
    /// The fee instruction will lock up the "max_fee" amount for the duration of the transaction.
    pub fn fee_transaction_pay_from_component<C: Into<ComponentCall>, A: Into<Amount>>(
        self,
        call: C,
        max_fee: A,
    ) -> Self {
        self.add_fee_instruction(Instruction::CallMethod {
            call: call.into(),
            method: "pay_fee".to_string(),
            args: call_args![max_fee.into().non_negative_checked().expect("Negative fee not allowed")],
        })
    }

    /// Adds a fee instruction that calls the "pay_fee_stealth" method on a component.
    /// This method should call either `Vault::pay_fee_stealth` or `ResourceManager::pay_fee_stealth` and result in a
    /// sufficient amount of revealed funds used to pay fees.
    pub fn fee_transaction_pay_fees_stealth_from_component<A: Into<ComponentCall>>(
        self,
        call: A,
        statement: StealthTransferStatement,
    ) -> Self {
        self.add_fee_instruction(Instruction::CallMethod {
            call: call.into(),
            method: "pay_fee_stealth".to_string(),
            args: call_args![statement],
        })
    }

    pub fn create_account(self, owner_public_key: RistrettoPublicKeyBytes) -> Self {
        self.add_instruction(Instruction::CreateAccount {
            owner_public_key,
            owner_rule: None,
            access_rules: None,
            workspace_id: None,
        })
    }

    pub fn create_account_with_bucket<T: Into<BuilderWorkspaceKey>>(
        self,
        owner_public_key: RistrettoPublicKeyBytes,
        workspace_id: T,
    ) -> Self {
        let workspace_id = self.get_workspace_offset_id_from_named_arg(workspace_id);
        self.add_instruction(Instruction::CreateAccount {
            owner_public_key,
            owner_rule: None,
            access_rules: None,
            workspace_id: Some(workspace_id),
        })
    }

    pub fn create_account_with_custom_rules<T: Into<BuilderWorkspaceKey>>(
        self,
        public_key_address: RistrettoPublicKeyBytes,
        owner_rule: Option<OwnerRule>,
        access_rules: Option<AccessRules>,
        workspace_id: Option<T>,
    ) -> Self {
        let workspace_id = workspace_id.map(|id| self.get_workspace_offset_id_from_named_arg(id));
        self.add_instruction(Instruction::CreateAccount {
            owner_public_key: public_key_address,
            owner_rule,
            access_rules,
            workspace_id,
        })
    }

    pub fn call_function<T: Into<String>>(
        self,
        template_address: TemplateAddress,
        function: T,
        args: Vec<NamedArg>,
    ) -> Self {
        let args = self.resolve_args(args).expect("Invalid named arguments");
        self.add_instruction(Instruction::CallFunction {
            address: template_address,
            function: function.into(),
            args,
        })
    }

    pub fn call_method<A: Into<NamedComponentCall>, T: Into<String>>(
        self,
        call: A,
        method: T,
        args: Vec<NamedArg>,
    ) -> Self {
        let call = self.resolve_call(call.into());
        let args = self.resolve_args(args).expect("Invalid named arguments");
        self.add_instruction(Instruction::CallMethod {
            call,
            method: method.into(),
            args,
        })
    }

    pub fn stealth_transfer<R: Into<NamedResourceRef>>(self, resource: R, statement: StealthTransferStatement) -> Self {
        self.stealth_transfer_with_opt_bucket(resource, statement, None::<String>)
    }

    pub fn stealth_transfer_with_input_bucket<B: Into<String>, R: Into<NamedResourceRef>>(
        self,
        resource_address: R,
        statement: StealthTransferStatement,
        bucket: B,
    ) -> Self {
        self.stealth_transfer_with_opt_bucket(resource_address, statement, Some(bucket))
    }

    pub fn stealth_transfer_with_opt_bucket<B: Into<String>, R: Into<NamedResourceRef>>(
        self,
        resource: R,
        statement: StealthTransferStatement,
        bucket: Option<B>,
    ) -> Self {
        let resource_address = self.resolve_resource_ref(resource.into());
        let revealed_input_bucket = bucket.map(|s| self.get_workspace_offset_id_from_named_arg(s));
        self.add_instruction(Instruction::StealthTransfer {
            resource_address_ref: resource_address,
            statement,
            revealed_input_bucket,
        })
    }

    pub fn pay_fee_stealth(self, statement: StealthTransferStatement) -> Self {
        self.pay_fee_stealth_with_opt_input_bucket(statement, None::<String>)
    }

    pub fn pay_fee_stealth_with_input_bucket<B: Into<String>>(
        self,
        statement: StealthTransferStatement,
        input_bucket: B,
    ) -> Self {
        self.pay_fee_stealth_with_opt_input_bucket(statement, Some(input_bucket))
    }

    pub fn pay_fee_stealth_with_opt_input_bucket<B: Into<String>>(
        self,
        statement: StealthTransferStatement,
        input_bucket: Option<B>,
    ) -> Self {
        let revealed_input_bucket = input_bucket.map(|bucket| self.get_workspace_offset_id_from_named_arg(bucket));
        self.add_instruction(Instruction::PayFee {
            statement,
            revealed_input_bucket,
        })
    }

    pub fn drop_all_proofs_in_workspace(self) -> Self {
        self.add_instruction(Instruction::DropAllProofsInWorkspace)
    }

    pub fn put_last_instruction_output_on_workspace<T: Into<BuilderWorkspaceKey>>(mut self, label: T) -> Self {
        let key = self.workspace_ids.insert(label.into());
        self.add_instruction(Instruction::PutLastInstructionOutputOnWorkspace { key })
    }

    pub fn assert_bucket_contains<T: AsRef<str>, A: Into<Amount>>(
        self,
        label: T,
        resource_address: ResourceAddress,
        min_amount: A,
    ) -> Self {
        let key = self.get_workspace_offset_id_from_named_arg(label.as_ref());
        self.add_instruction(Instruction::AssertBucketContains {
            key,
            resource_address,
            min_amount: min_amount.into(),
        })
    }

    /// Publishing a WASM template.
    pub fn publish_template(self, binary: Vec<u8>) -> Self {
        self.add_instruction(Instruction::PublishTemplate { binary })
    }

    pub fn claim_burn(self, claim: MinotariBurnClaimProof, output_data: ClaimBurnOutputData) -> Self {
        self.add_instruction(Instruction::ClaimBurn {
            claim: Box::new(claim),
            output_data,
        })
    }

    pub fn claim_validator_fees(self, address: ValidatorFeePoolAddress) -> Self {
        self.add_instruction(Instruction::ClaimValidatorFees { address })
    }

    pub fn create_proof<A: Into<ComponentCall>>(self, account: A, resource_addr: ResourceAddress) -> Self {
        // We may want to make this a native instruction
        self.add_instruction(Instruction::CallMethod {
            call: account.into(),
            method: "create_proof_for_resource".to_string(),
            args: call_args![resource_addr],
        })
    }

    pub fn with_fee_instructions<I: IntoIterator<Item = Instruction>>(mut self, instructions: I) -> Self {
        self.unsigned_transaction.fee_instructions_mut().extend(instructions);
        // Reset the signatures as they are no longer valid
        self.clear_signatures();
        self
    }

    pub fn with_fee_instructions_builder<F: FnOnce(TransactionBuilder) -> TransactionBuilder>(mut self, f: F) -> Self {
        // TODO: pass in a fee builder type (probably TransactionBuilder<FeeBuilder> which has applicable methods)
        let builder = f(TransactionBuilder::new());
        self.unsigned_transaction
            .fee_instructions_mut()
            .extend(builder.unsigned_transaction.into_instructions());
        // Reset the signatures as they are no longer valid
        self.clear_signatures();
        self
    }

    pub fn add_fee_instruction(mut self, instruction: Instruction) -> Self {
        self.unsigned_transaction.fee_instructions_mut().push(instruction);
        // Reset the signatures as they are no longer valid
        self.clear_signatures();
        self
    }

    pub fn add_instruction(mut self, instruction: Instruction) -> Self {
        self.unsigned_transaction.instructions_mut().push(instruction);
        // Reset the signatures as they are no longer valid
        self.clear_signatures();
        self
    }

    pub fn with_instructions<I: IntoIterator<Item = Instruction>>(mut self, instructions: I) -> Self {
        self.unsigned_transaction.instructions_mut().extend(instructions);
        // Reset the signatures as they are no longer valid
        self.clear_signatures();
        self
    }

    /// Add an input to use in the transaction
    pub fn add_input<I: Into<SubstateRequirement>>(mut self, input_object: I) -> Self {
        self.unsigned_transaction.inputs_mut().insert(input_object.into());
        // Reset the signatures as they are no longer valid
        self.clear_signatures();
        self
    }

    pub fn with_inputs<I: IntoIterator<Item = SubstateRequirement>>(mut self, inputs: I) -> Self {
        self.unsigned_transaction = self.unsigned_transaction.with_inputs(inputs);
        // Reset the signatures as they are no longer valid
        self.clear_signatures();
        self
    }

    pub fn with_unversioned_inputs<I: IntoIterator<Item = S>, S: Into<SubstateId>>(self, inputs: I) -> Self {
        self.with_inputs(inputs.into_iter().map(|input| SubstateRequirement::unversioned(input)))
    }

    pub fn with_min_epoch(mut self, min_epoch: Option<Epoch>) -> Self {
        self.unsigned_transaction.set_min_epoch(min_epoch);
        // Reset the signatures as they are no longer valid
        self.clear_signatures();
        self
    }

    pub fn with_max_epoch(mut self, max_epoch: Option<Epoch>) -> Self {
        self.unsigned_transaction.set_max_epoch(max_epoch);
        // Reset the signatures as they are no longer valid
        self.clear_signatures();
        self
    }

    /// Pre-allocate a component address. The allocated address is added to the workspace and can be used in subsequent
    /// instructions.
    pub fn allocate_component_address<T: Into<BuilderWorkspaceKey>>(mut self, workspace_id: T) -> Self {
        // Note: offset syntax does not make sense when adding something to the workspace and is not supported by the
        // engine
        let workspace_id = self.workspace_ids.insert(workspace_id.into());
        self.add_instruction(Instruction::AllocateAddress {
            allocatable_type: AllocatableAddressType::Component,
            workspace_id,
        })
    }

    /// Pre-allocate a resource address. The allocated address is added to the workspace and can be used in subsequent
    /// instructions.
    pub fn allocate_resource_address<T: Into<String>>(mut self, workspace_id: T) -> Self {
        // Note: offset syntax does not make sense when adding something to the workspace and is not supported by the
        // engine
        let workspace_id = self.workspace_ids.insert(workspace_id.into());
        self.add_instruction(Instruction::AllocateAddress {
            allocatable_type: AllocatableAddressType::Resource,
            workspace_id,
        })
    }

    pub fn build_unsigned_transaction(self) -> UnsignedTransaction {
        self.unsigned_transaction
    }

    pub fn add_signer(self, sealed_signer: &RistrettoPublicKeyBytes, secret_key: &RistrettoSecretKey) -> Self {
        let signature = match &self.unsigned_transaction {
            UnsignedTransaction::V1(tx) => TransactionSignature::sign_v1(secret_key, sealed_signer, tx),
        };
        self.add_signature(signature)
    }

    pub fn add_signature(mut self, signature: TransactionSignature) -> Self {
        self.signatures.push(signature);
        self
    }

    pub fn signatures(&self) -> &[TransactionSignature] {
        &self.signatures
    }

    fn clear_signatures(&mut self) {
        self.signatures = vec![];
    }

    pub fn build(self) -> UnsealedTransactionV1 {
        let builder = self.then(|builder| {
            // This is so that we dont have to add this in a lot of places - TODO: this is an assumption that may not
            // apply to all transactions
            if builder.signatures.is_empty() {
                builder.with_authorized_seal_signer()
            } else {
                builder
            }
        });

        builder.unsigned_transaction.build(builder.signatures)
    }

    pub fn build_and_seal(self, secret_key: &RistrettoSecretKey) -> Transaction {
        self.build().seal(secret_key)
    }

    fn resolve_call(&self, call: NamedComponentCall) -> ComponentCall {
        match call {
            NamedComponentCall::Address(call) => call.into(),
            NamedComponentCall::Workspace(call) => {
                let id = self.workspace_ids.get(call.name()).unwrap_or_else(|| {
                    panic!("Workspace key '{}' not found", call.name());
                });
                ComponentCall::Workspace(id)
            },
        }
    }

    fn resolve_resource_ref(&self, resx_ref: NamedResourceRef) -> ResourceAddressRef {
        match resx_ref {
            NamedResourceRef::Address(addr) => addr.into(),
            NamedResourceRef::Workspace(id) => {
                let id = self.workspace_ids.get(id.name()).unwrap_or_else(|| {
                    panic!("Workspace key '{}' not found", id.name());
                });
                WorkspaceOffsetId::new(id).into()
            },
        }
    }

    /// Maps named arguments to the template_lib workspace or literal args.
    fn resolve_args(&self, args: Vec<NamedArg>) -> Result<Vec<InstructionArg>, ParseWorkspaceKeyError> {
        args.into_iter().map(|arg| self.resolve_arg(arg)).collect()
    }

    fn resolve_arg(&self, arg: NamedArg) -> Result<InstructionArg, ParseWorkspaceKeyError> {
        match arg {
            NamedArg::Literal(bytes) => Ok(InstructionArg::Literal(bytes)),
            NamedArg::Workspace(key) => {
                let parsed = parse_workspace_key(key)?;
                let id = self.workspace_ids.get(parsed.name.as_ref()).unwrap_or_else(|| {
                    panic!("Workspace key '{}' not found", parsed.name);
                });
                Ok(InstructionArg::Workspace(
                    WorkspaceOffsetId::new(id).with_offset_opt(parsed.offset),
                ))
            },
        }
    }

    pub fn get_workspace_offset_id_from_named_arg<T: Into<String>>(&self, id: T) -> WorkspaceOffsetId {
        let parsed = parse_workspace_key(id.into()).expect("Invalid workspace key format");
        let Some(id) = self.workspace_ids.get(&parsed.name) else {
            panic!("Workspace key '{}' not found", parsed.name);
        };
        WorkspaceOffsetId::new(id).with_offset_opt(parsed.offset)
    }
}
