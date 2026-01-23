//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod error;
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
    indexed_value::IndexedValue,
    published_template::TemplateBlob,
    substate::SubstateId,
};
use tari_ootle_common_types::{Epoch, SubstateRequirement};
use tari_template_lib::{
    prelude::{AccessRules, StealthTransferStatement},
    types::{
        constants::XTR,
        crypto::RistrettoPublicKeyBytes,
        Amount,
        FunctionName,
        OwnerRule,
        ResourceAddress,
        TemplateAddress,
        ValidatorFeePoolAddress,
    },
};

use crate::{
    args,
    args::{InstructionArg, WorkspaceOffsetId},
    builder::{
        error::BuilderError,
        named_args::{parse_workspace_key, BuilderWorkspaceKey, NamedArg},
        named_resource_ref::NamedResourceRef,
        workspace_ids::WorkspaceIds,
    },
    call_args,
    unsigned_transaction::UnsignedTransaction,
    AllocatableAddressType,
    ComponentReference,
    Instruction,
    IntoSigned,
    MigrateFunction,
    ResourceAddressRef,
    Signable,
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
    fill_inputs: bool,
}

impl TransactionBuilder<MainIntent> {
    pub fn new<N: Into<u8>>(network: N) -> Self {
        let network = network.into();
        Self {
            unsigned_transaction: UnsignedTransaction::new(network),
            workspace_ids: WorkspaceIds::new(),
            fee_instruction_builder: Some(Box::new(Self::new_fee_builder(network))),
            _discriminator: std::marker::PhantomData,
            fill_inputs: false,
        }
    }

    pub fn with_unsigned_transaction<T: Into<UnsignedTransaction>>(self, unsigned_transaction: T) -> Self {
        let unsigned_transaction = unsigned_transaction.into();
        Self {
            fee_instruction_builder: Some(Box::new(Self::new_fee_builder(unsigned_transaction.network()))),
            unsigned_transaction,
            workspace_ids: WorkspaceIds::new(),
            _discriminator: std::marker::PhantomData,
            fill_inputs: false,
        }
    }

    fn new_fee_builder<N: Into<u8>>(network: N) -> TransactionBuilder<FeeIntent> {
        TransactionBuilder {
            unsigned_transaction: UnsignedTransaction::new(network),
            workspace_ids: WorkspaceIds::new(),
            fee_instruction_builder: None,
            _discriminator: std::marker::PhantomData,
            fill_inputs: false,
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

    pub fn with_fee_instructions<I>(self, instructions: I) -> Self
    where
        I: IntoIterator<Item = Instruction>,
        I::IntoIter: ExactSizeIterator,
    {
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

    pub fn update_component_template_address_with_migrate<C, T>(
        self,
        component: C,
        new_template: TemplateAddress,
        migrate_name: T,
        migrate_args: Vec<NamedArg>,
    ) -> Self
    where
        C: Into<NamedComponentCall>,
        T: TryInto<FunctionName>,
        <T as TryInto<FunctionName>>::Error: std::fmt::Debug,
    {
        let component = self.resolve_call(component.into());
        let migrate_args = self.resolve_args(migrate_args).expect("Invalid named arguments");
        self.add_instruction(Instruction::UpdateComponentTemplate {
            component,
            migrate: Some(MigrateFunction {
                name: migrate_name
                    .try_into()
                    .expect("Oops! The provided migrate function name is longer than the limit"),
                args: migrate_args,
            }),
            new_template,
        })
    }

    pub fn update_component_template_address<C: Into<NamedComponentCall>>(
        self,
        component: C,
        new_template: TemplateAddress,
    ) -> Self {
        let component = self.resolve_call(component.into());
        self.add_instruction(Instruction::UpdateComponentTemplate {
            component,
            migrate: None,
            new_template,
        })
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

    pub fn with_signatures(self, signatures: Vec<TransactionSignature>) -> UnsealedTransactionV1 {
        self.build_unsigned_transaction().with_signatures(signatures)
    }

    /// Moves the fee instructions from the fee builder into the unsigned transaction.
    fn apply_fee_instructions(&mut self) {
        let mut fee_builder = self
            .fee_instruction_builder
            .take()
            .expect("Fee instruction builder is None");
        self.unsigned_transaction
            .inputs_mut()
            .extend(fee_builder.unsigned_transaction.inputs_mut().drain(..));
        self.unsigned_transaction
            .fee_instructions_mut()
            .extend(fee_builder.unsigned_transaction.into_instructions());
    }

    pub fn finish(mut self) -> UnsealedTransactionV1 {
        self.apply_fee_instructions();
        self.unsigned_transaction.finish()
    }

    /// Returns the instructions in the transaction. WARNING: Fee instructions are discarded.
    pub fn into_instructions(self) -> Vec<Instruction> {
        self.unsigned_transaction.into_instructions()
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
    pub fn next_workspace_id(&self) -> args::WorkspaceId {
        self.workspace_ids.next_id()
    }

    pub fn with_auto_fill_inputs(mut self) -> Self {
        self.fill_inputs = true;
        if let Some(ref mut builder_mut) = self.fee_instruction_builder {
            builder_mut.fill_inputs = true;
        }
        self
    }

    pub fn without_auto_fill_inputs(mut self) -> Self {
        self.fill_inputs = false;
        if let Some(ref mut builder_mut) = self.fee_instruction_builder {
            builder_mut.fill_inputs = false;
        }
        self
    }

    pub fn for_network<N: Into<u8>>(mut self, network: N) -> Self {
        self.unsigned_transaction.set_network(network);
        self
    }

    pub fn then<F: FnOnce(Self) -> Self>(self, f: F) -> Self {
        f(self)
    }

    pub fn map<F: FnOnce(Self) -> T, T>(self, f: F) -> T {
        f(self)
    }

    /// Adds a CreateAccount instruction to the transaction.
    /// Note that CreateAccount is idempotent, so can be called regardless of whether the account already exists or not.
    /// If it does exist, this instruction will be a no-op.
    pub fn create_account(self, owner_public_key: RistrettoPublicKeyBytes) -> Self {
        self.add_instruction(Instruction::CreateAccount {
            owner_public_key,
            owner_rule: None,
            access_rules: None,
            bucket_workspace_id: None,
        })
    }

    /// Adds a CreateAccount instruction to the transaction, depositing the bucket into the newly created account.
    /// Note that CreateAccount is idempotent, so can be called regardless of whether the account already exists or not.
    /// If it does exist, this instruction will deposit the bucket into the existing account.
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
            bucket_workspace_id: Some(workspace_id),
        })
    }

    pub fn create_account_custom<T: Into<BuilderWorkspaceKey>>(
        self,
        public_key_address: RistrettoPublicKeyBytes,
        owner_rule: Option<OwnerRule>,
        access_rules: Option<AccessRules>,
        bucket_workspace_id: Option<T>,
    ) -> Self {
        let bucket_workspace_id = bucket_workspace_id.map(|id| self.get_workspace_offset_id_from_named_arg(id));
        self.add_instruction(Instruction::CreateAccount {
            owner_public_key: public_key_address,
            owner_rule,
            access_rules,
            bucket_workspace_id,
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

    fn add_input_for_component_ref(&mut self, component_ref: &ComponentReference) {
        if self.fill_inputs {
            if let Some(address) = component_ref.address() {
                self.unsigned_transaction.inputs_mut().insert((*address).into());
            }
        }
    }

    fn add_input_for_resource_ref(&mut self, resource_ref: &ResourceAddressRef) {
        if self.fill_inputs {
            if let ResourceAddressRef::Address(address) = resource_ref {
                self.add_resource_input(*address);
            }
        }
    }

    fn add_resource_input(&mut self, resource: ResourceAddress) -> &mut Self {
        // XTR is implicit
        if self.fill_inputs && resource != XTR {
            self.unsigned_transaction.inputs_mut().insert(resource.into());
        }
        self
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
        mut self,
        resource: R,
        statement: StealthTransferStatement,
        bucket: Option<B>,
    ) -> Self {
        let resource_address = self.resolve_resource_ref(resource.into());
        self.add_input_for_resource_ref(&resource_address);
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
        mut self,
        label: T,
        resource_address: ResourceAddress,
        min_amount: A,
    ) -> Self {
        let key = self.get_workspace_offset_id_from_named_arg(label.as_ref());
        self.add_resource_input(resource_address);

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
        self.then(|b| if b.fill_inputs { b.add_input(address) } else { b })
            .add_instruction(Instruction::ClaimValidatorFees { address })
    }

    pub fn create_proof<A: Into<ComponentReference>>(mut self, account: A, resource_addr: ResourceAddress) -> Self {
        let component_ref = account.into();
        self.add_input_for_component_ref(&component_ref);
        self.add_resource_input(resource_addr);
        // We may want to make this a native instruction
        self.add_instruction(Instruction::CallMethod {
            call: component_ref,
            method: "create_proof_for_resource"
                .try_into()
                .expect("Method name is longer than the limit"),
            args: call_args![resource_addr],
        })
    }

    pub fn add_instruction(mut self, instruction: Instruction) -> Self {
        let id = instruction.allocated_workspace_id();
        if let Some(id) = id {
            // + 1 because the current id counter is the next available id
            let next_id = id.checked_add(1).expect("Workspace ID overflow");
            if next_id > self.workspace_ids.next_id() {
                self.workspace_ids.set_next_id(next_id);
            }
        }
        self.add_inputs_for_instruction(&instruction);
        self.unsigned_transaction.instructions_mut().push(instruction);
        self
    }

    /// Adds multiple instructions to the transaction.
    /// NOTE: the instruction args are not resolved here, so any workspace references must resolved manually by the
    /// caller and cannot be referenced in subsequent instructions.
    /// Instructions which allocate workspace IDs will update the internal workspace ID counter accordingly.
    pub fn with_instructions<I>(mut self, instructions: I) -> Self
    where
        I: IntoIterator<Item = Instruction>,
        I::IntoIter: ExactSizeIterator,
    {
        let iter = instructions.into_iter();
        self.unsigned_transaction.instructions_mut().reserve(iter.len());
        for instruction in iter {
            self = self.add_instruction(instruction)
        }
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

    fn add_inputs_for_instruction(&mut self, instruction: &Instruction) {
        if !self.fill_inputs {
            return;
        }

        match instruction {
            Instruction::CallFunction { args, .. } => {
                for arg in args {
                    if let InstructionArg::Literal(bytes) = arg {
                        if let Ok(indexed) = IndexedValue::from_raw(bytes) {
                            self.unsigned_transaction
                                .inputs_mut()
                                .extend(indexed.referenced_substates().map(SubstateRequirement::unversioned));
                        }
                    }
                }
            },
            Instruction::CallMethod { call, args, .. } => {
                self.add_input_for_component_ref(call);
                for arg in args {
                    if let InstructionArg::Literal(bytes) = arg {
                        if let Ok(indexed) = IndexedValue::from_raw(bytes) {
                            self.unsigned_transaction
                                .inputs_mut()
                                .extend(indexed.referenced_substates().map(SubstateRequirement::unversioned));
                        }
                    }
                }
            },
            Instruction::StealthTransfer {
                resource_address_ref, ..
            } => {
                self.add_input_for_resource_ref(resource_address_ref);
            },
            Instruction::ClaimValidatorFees { address } => {
                self.unsigned_transaction.inputs_mut().insert((*address).into());
            },
            Instruction::AssertBucketContains { resource_address, .. } => {
                if *resource_address != XTR {
                    self.unsigned_transaction
                        .inputs_mut()
                        .insert((*resource_address).into());
                }
            },
            Instruction::UpdateComponentTemplate { component, .. } => {
                self.add_input_for_component_ref(component);
            },
            Instruction::CreateAccount { .. } |
            Instruction::PutLastInstructionOutputOnWorkspace { .. } |
            Instruction::EmitLog { .. } |
            Instruction::ClaimBurn { .. } |
            Instruction::DropAllProofsInWorkspace |
            Instruction::TakeFromBucket { .. } |
            Instruction::PublishTemplate { .. } |
            Instruction::AllocateAddress { .. } |
            Instruction::PayFee { .. } => {},
        }
    }

    fn resolve_call(&self, call: NamedComponentCall) -> ComponentReference {
        match call {
            NamedComponentCall::Address(call) => call.into(),
            NamedComponentCall::Workspace(call) => {
                let id = self.workspace_ids.get(call.name()).unwrap_or_else(|| {
                    panic!("Workspace key '{}' not found", call.name());
                });
                ComponentReference::Workspace(id)
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
    fn resolve_args(&self, args: Vec<NamedArg>) -> Result<Vec<InstructionArg>, BuilderError> {
        args.into_iter().map(|arg| self.resolve_arg(arg)).collect()
    }

    fn resolve_arg(&self, arg: NamedArg) -> Result<InstructionArg, BuilderError> {
        match arg {
            NamedArg::Literal(bytes) => Ok(InstructionArg::Literal(bytes.into())),
            NamedArg::Workspace(key) => {
                let parsed = parse_workspace_key(key)?;
                let id = self
                    .workspace_ids
                    .get(parsed.name.as_ref())
                    .ok_or(BuilderError::WorkspaceKeyNotFound(parsed.name))?;
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
    type Signature = TransactionSignature;

    fn to_signing_message(&self, sealed_signer: &RistrettoPublicKeyBytes) -> Self::MessageOutput {
        self.unsigned_transaction.to_signing_message(sealed_signer)
    }
}

impl IntoSigned<&RistrettoPublicKeyBytes> for TransactionBuilder<MainIntent> {
    type SignedOutput = UnsealedTransactionV1;

    fn into_signed(self, sig: TransactionSignature) -> Self::SignedOutput {
        self.finish().add_signature(sig)
    }
}
