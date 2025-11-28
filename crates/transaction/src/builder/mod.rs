//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub mod named_args;
mod named_component_call;
mod named_resource_ref;
#[cfg(test)]
mod tests;
mod workspace_ids;

pub use named_component_call::*;
use tari_crypto::ristretto::{RistrettoPublicKey, RistrettoSchnorr, RistrettoSecretKey};
use tari_engine_types::{
    confidential::{ClaimBurnOutputData, MinotariBurnClaimProof},
    published_template::TemplateBlob,
    substate::SubstateId,
    ToByteType,
    ValidatorFeePoolAddress,
};
use tari_ootle_common_types::{Epoch, IntoSigned, Signable, SubstateRequirement};
use tari_template_lib::{
    auth::OwnerRule,
    models::{ResourceAddress, StealthTransferStatement},
    prelude::AccessRules,
    types::{crypto::RistrettoPublicKeyBytes, Amount, FunctionName, TemplateAddress},
};

use crate::{
    args,
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

#[derive(Debug, Clone)]
pub enum MainIntent {}
#[derive(Debug, Clone)]
pub enum FeeIntent {}

#[derive(Debug, Clone)]
pub struct TransactionBuilder<D = MainIntent> {
    unsigned_transaction: UnsignedTransaction,
    workspace_ids: WorkspaceIds,
    fee_instruction_builder: Option<Box<TransactionBuilder<FeeIntent>>>,
    _discriminator: std::marker::PhantomData<D>,
}

impl TransactionBuilder<MainIntent> {
    pub fn new<N: Into<u8>>(network: N) -> Self {
        let network = network.into();
        Self {
            unsigned_transaction: UnsignedTransaction::new(network),
            workspace_ids: WorkspaceIds::new(),
            fee_instruction_builder: Some(Box::new(Self::new_fee_builder(network))),
            _discriminator: std::marker::PhantomData,
        }
    }

    pub fn with_unsigned_transaction<T: Into<UnsignedTransaction>>(self, unsigned_transaction: T) -> Self {
        let unsigned_transaction = unsigned_transaction.into();
        Self {
            fee_instruction_builder: Some(Box::new(Self::new_fee_builder(unsigned_transaction.network()))),
            unsigned_transaction,
            workspace_ids: WorkspaceIds::new(),
            _discriminator: std::marker::PhantomData,
        }
    }

    pub fn for_network<N: Into<u8>>(mut self, network: N) -> Self {
        self.unsigned_transaction.set_network(network);
        self
    }

    fn new_fee_builder<N: Into<u8>>(network: N) -> TransactionBuilder<FeeIntent> {
        TransactionBuilder {
            unsigned_transaction: UnsignedTransaction::new(network),
            workspace_ids: WorkspaceIds::new(),
            fee_instruction_builder: None,
            _discriminator: std::marker::PhantomData,
        }
    }

    /// Pays fees using a stealth transfer statement. The statement must reveal sufficient funds to cover the fee.
    /// NOTE: fees paid are not refunded, so any overpayment is kept by validators.
    pub fn pay_fee_stealth(self, statement: StealthTransferStatement) -> Self {
        self.with_fee_instructions_builder(|builder| builder.pay_fee_stealth(statement))
    }

    /// Adds a fee instruction that calls the "take_fee" method on a component.
    /// This method must exist and return a Bucket with containing revealed confidential XTR resource.
    /// This allows the fee to originate from sources other than the transaction sender's account.
    /// The fee instruction will lock up the "max_fee" amount for the duration of the transaction.
    pub fn pay_fee_from_component<C: Into<NamedComponentCall>, A: Into<Amount>>(self, call: C, max_fee: A) -> Self {
        self.with_fee_instructions_builder(|builder| builder.pay_fee_from_component(call, max_fee))
    }

    /// Adds a fee instruction that calls the "pay_fee_stealth" method on a component.
    /// This method should call either `Vault::pay_fee_stealth` or `ResourceManager::pay_fee_stealth` and result in a
    /// sufficient amount of revealed funds used to pay fees.
    pub fn pay_fee_stealth_from_component<A: Into<NamedComponentCall>>(
        self,
        call: A,
        statement: StealthTransferStatement,
    ) -> Self {
        self.with_fee_instructions_builder(|builder| builder.call_method(call, "pay_fee_stealth", args![statement]))
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
        self.with_fee_instructions_builder(|builder| {
            builder.pay_fee_stealth_with_opt_input_bucket(statement, input_bucket)
        })
    }

    pub fn with_fee_instructions<I: IntoIterator<Item = Instruction>>(self, instructions: I) -> Self {
        self.with_fee_instructions_builder(|builder| builder.with_instructions(instructions))
    }

    pub fn with_fee_instructions_builder<F: FnOnce(TransactionBuilder<FeeIntent>) -> TransactionBuilder<FeeIntent>>(
        mut self,
        f: F,
    ) -> Self {
        let builder = f(*self.fee_instruction_builder.take().unwrap());
        self.fee_instruction_builder = Some(Box::new(builder));
        self
    }

    pub fn add_fee_instruction(self, instruction: Instruction) -> Self {
        self.with_fee_instructions_builder(|builder| builder.add_instruction(instruction))
    }

    /// Add an input to use in the transaction
    pub fn add_input<I: Into<SubstateRequirement>>(mut self, input_object: I) -> Self {
        self.unsigned_transaction.inputs_mut().insert(input_object.into());
        self
    }

    pub fn with_inputs<I: IntoIterator<Item = SubstateRequirement>>(mut self, inputs: I) -> Self {
        self.unsigned_transaction = self.unsigned_transaction.with_inputs(inputs);
        self
    }

    pub fn with_unversioned_inputs<I: IntoIterator<Item = S>, S: Into<SubstateId>>(self, inputs: I) -> Self {
        self.with_inputs(inputs.into_iter().map(|input| SubstateRequirement::unversioned(input)))
    }

    pub fn with_min_epoch(mut self, min_epoch: Option<Epoch>) -> Self {
        self.unsigned_transaction.set_min_epoch(min_epoch);
        self
    }

    pub fn with_max_epoch(mut self, max_epoch: Option<Epoch>) -> Self {
        self.unsigned_transaction.set_max_epoch(max_epoch);
        self
    }

    pub fn add_signer(
        self,
        sealed_signer: &RistrettoPublicKeyBytes,
        secret_key: &RistrettoSecretKey,
    ) -> UnsealedTransactionV1 {
        let unsigned = self.build_unsigned_transaction();
        let signature = TransactionSignature::sign(secret_key, sealed_signer, &unsigned);
        unsigned.add_signature(signature)
    }

    pub fn add_signature(self, signature: TransactionSignature) -> UnsealedTransactionV1 {
        self.build_unsigned_transaction().add_signature(signature)
    }

    fn apply_fee_instructions(&mut self) {
        let fee_builder = self
            .fee_instruction_builder
            .take()
            .expect("Fee instruction builder is None");
        self.unsigned_transaction
            .fee_instructions_mut()
            .extend(fee_builder.unsigned_transaction.into_instructions());
    }

    pub fn finish(mut self) -> UnsealedTransactionV1 {
        self.apply_fee_instructions();
        self.unsigned_transaction.finish()
    }

    pub fn build_and_seal(self, secret_key: &RistrettoSecretKey) -> Transaction {
        self.finish().seal(secret_key)
    }

    pub fn with_dry_run(mut self, dry_run: bool) -> Self {
        self.unsigned_transaction.set_dry_run(dry_run);
        self
    }

    pub fn with_disabled_seal_signer_authorization(mut self) -> Self {
        self.unsigned_transaction = self.unsigned_transaction.disabled_authorized_sealed_signer();
        self
    }

    pub fn build_unsigned_transaction(mut self) -> UnsignedTransaction {
        self.apply_fee_instructions();
        self.unsigned_transaction
    }
}

impl TransactionBuilder<FeeIntent> {
    /// Pays fees using a stealth transfer statement. The statement must reveal sufficient funds to cover the fee.
    /// NOTE: fees paid are not refunded, so any overpayment is kept by validators.
    pub fn pay_fee_stealth(self, statement: StealthTransferStatement) -> Self {
        self.add_instruction(Instruction::PayFee {
            statement,
            revealed_input_bucket: None,
        })
    }

    /// Adds a fee instruction that calls the "take_fee" method on a component.
    /// This method must exist and return a Bucket with containing revealed confidential XTR resource.
    /// This allows the fee to originate from sources other than the transaction sender's account.
    /// The fee instruction will lock up the "max_fee" amount for the duration of the transaction.
    pub fn pay_fee_from_component<C: Into<NamedComponentCall>, A: Into<Amount>>(self, call: C, max_fee: A) -> Self {
        self.call_method(call, "pay_fee", args![max_fee.into()])
    }

    /// Adds a fee instruction that calls the "pay_fee_stealth" method on a component.
    /// This method should call either `Vault::pay_fee_stealth` or `ResourceManager::pay_fee_stealth` and result in a
    /// sufficient amount of revealed funds used to pay fees.
    pub fn pay_fee_stealth_from_component<A: Into<NamedComponentCall>>(
        self,
        call: A,
        statement: StealthTransferStatement,
    ) -> Self {
        self.call_method(call, "pay_fee_stealth", args![statement])
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
}

impl<D> TransactionBuilder<D> {
    pub fn then<F: FnOnce(Self) -> Self>(self, f: F) -> Self {
        f(self)
    }

    pub fn map<F: FnOnce(Self) -> T, T>(self, f: F) -> T {
        f(self)
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

    pub fn call_function<T>(self, template_address: TemplateAddress, function: T, args: Vec<NamedArg>) -> Self
    where
        T: TryInto<FunctionName>,
        <T as TryInto<FunctionName>>::Error: std::fmt::Debug,
    {
        let args = self.resolve_args(args).expect("Invalid named arguments");
        self.add_instruction(Instruction::CallFunction {
            address: template_address,
            function: function
                .try_into()
                .expect("Oops! The provided function name is longer than the limit"),
            args,
        })
    }

    pub fn call_method<C, T>(self, call: C, method: T, args: Vec<NamedArg>) -> Self
    where
        C: Into<NamedComponentCall>,
        T: TryInto<FunctionName>,
        <T as TryInto<FunctionName>>::Error: std::fmt::Debug,
    {
        let call = self.resolve_call(call.into());
        let args = self.resolve_args(args).expect("Invalid named arguments");
        self.add_instruction(Instruction::CallMethod {
            call,
            method: method
                .try_into()
                .expect("Oops! The provided method name is longer than the limit"),
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

    pub fn drop_all_proofs_in_workspace(self) -> Self {
        self.add_instruction(Instruction::DropAllProofsInWorkspace)
    }

    pub fn put_last_instruction_output_on_workspace<T: Into<BuilderWorkspaceKey>>(mut self, label: T) -> Self {
        let key = self.workspace_ids.insert(label.into());
        self.add_instruction(Instruction::PutLastInstructionOutputOnWorkspace { key })
    }

    pub fn take_from_bucket<T: Into<BuilderWorkspaceKey>, A: Into<Amount>>(
        mut self,
        label: T,
        amount: A,
        output_label: T,
    ) -> Self {
        let key = self.get_workspace_offset_id_from_named_arg(label.into());
        let output_key = self.workspace_ids.insert(output_label.into());
        self.add_instruction(Instruction::TakeFromBucket {
            input_bucket: key,
            amount: amount.into(),
            output_bucket: output_key,
        })
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
    pub fn publish_template(self, binary: TemplateBlob) -> Self {
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
            method: "create_proof_for_resource"
                .try_into()
                .expect("Method name is longer than the limit"),
            args: call_args![resource_addr],
        })
    }

    pub fn add_instruction(mut self, instruction: Instruction) -> Self {
        self.unsigned_transaction.instructions_mut().push(instruction);
        self
    }

    pub fn with_instructions<I: IntoIterator<Item = Instruction>>(mut self, instructions: I) -> Self {
        self.unsigned_transaction.instructions_mut().extend(instructions);
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
            NamedArg::Literal(bytes) => Ok(InstructionArg::Literal(bytes.into())),
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

impl Signable<&RistrettoPublicKeyBytes> for TransactionBuilder<MainIntent> {
    type MessageOutput = [u8; 64];

    fn to_signing_message(&self, sealed_signer: &RistrettoPublicKeyBytes) -> Self::MessageOutput {
        self.unsigned_transaction.to_signing_message(sealed_signer)
    }
}

impl IntoSigned<&RistrettoPublicKeyBytes> for TransactionBuilder<MainIntent> {
    type SignedOutput = UnsealedTransactionV1;

    fn into_signed(self, public_key: RistrettoPublicKey, signature: RistrettoSchnorr) -> Self::SignedOutput {
        self.finish()
            .add_signature(public_key.to_byte_type(), signature.to_byte_type())
    }
}
