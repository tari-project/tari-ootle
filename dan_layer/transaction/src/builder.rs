//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_common_types::types::{PrivateKey, PublicKey};
use tari_dan_common_types::{Epoch, SubstateRequirement};
use tari_engine_types::{
    confidential::ConfidentialClaim,
    instruction::Instruction,
    vn_fee_pool::ValidatorFeePoolAddress,
    TemplateAddress,
};
use tari_template_lib::{
    args,
    args::Arg,
    auth::OwnerRule,
    models::{Amount, ComponentAddress, ConfidentialWithdrawProof, ResourceAddress},
    prelude::AccessRules,
};

use crate::{unsigned_transaction::UnsignedTransaction, Transaction, TransactionSignature, UnsealedTransactionV1};

#[derive(Debug, Clone, Default)]
pub struct TransactionBuilder {
    unsigned_transaction: UnsignedTransaction,
    signatures: Vec<TransactionSignature>,
}

impl TransactionBuilder {
    pub fn new() -> Self {
        Self {
            unsigned_transaction: UnsignedTransaction::default(),
            signatures: vec![],
        }
    }

    pub fn with_unsigned_transaction<T: Into<UnsignedTransaction>>(self, unsigned_transaction: T) -> Self {
        Self {
            unsigned_transaction: unsigned_transaction.into(),
            signatures: vec![],
        }
    }

    pub fn then<F: FnOnce(Self) -> Self>(self, f: F) -> Self {
        f(self)
    }

    pub fn for_network<N: Into<u8>>(mut self, network: N) -> Self {
        self.unsigned_transaction.set_network(network);
        self
    }

    pub fn with_authorized_seal_signer(mut self) -> Self {
        self.unsigned_transaction.authorized_sealed_signer();
        self.clear_signatures();
        self
    }

    /// Adds a fee instruction that calls the "take_fee" method on a component.
    /// This method must exist and return a Bucket with containing revealed confidential XTR resource.
    /// This allows the fee to originate from sources other than the transaction sender's account.
    /// The fee instruction will lock up the "max_fee" amount for the duration of the transaction.
    pub fn fee_transaction_pay_from_component(self, component_address: ComponentAddress, max_fee: Amount) -> Self {
        self.add_fee_instruction(Instruction::CallMethod {
            component_address,
            method: "pay_fee".to_string(),
            args: args![max_fee],
        })
    }

    /// Adds a fee instruction that calls the "take_fee_confidential" method on a component.
    /// This method must exist and return a Bucket with containing revealed confidential XTR resource.
    /// This allows the fee to originate from sources other than the transaction sender's account.
    pub fn fee_transaction_pay_from_component_confidential(
        self,
        component_address: ComponentAddress,
        proof: ConfidentialWithdrawProof,
    ) -> Self {
        self.add_fee_instruction(Instruction::CallMethod {
            component_address,
            method: "pay_fee_confidential".to_string(),
            args: args![proof],
        })
    }

    pub fn create_account(self, owner_public_key: PublicKey) -> Self {
        self.add_instruction(Instruction::CreateAccount {
            public_key_address: owner_public_key,
            owner_rule: None,
            access_rules: None,
            workspace_bucket: None,
        })
    }

    pub fn create_account_with_bucket<T: Into<String>>(self, owner_public_key: PublicKey, workspace_bucket: T) -> Self {
        self.add_instruction(Instruction::CreateAccount {
            public_key_address: owner_public_key,
            owner_rule: None,
            access_rules: None,
            workspace_bucket: Some(workspace_bucket.into()),
        })
    }

    pub fn create_account_with_custom_rules<T: Into<String>>(
        self,
        public_key_address: PublicKey,
        owner_rule: Option<OwnerRule>,
        access_rules: Option<AccessRules>,
        workspace_bucket: Option<T>,
    ) -> Self {
        self.add_instruction(Instruction::CreateAccount {
            public_key_address,
            owner_rule,
            access_rules,
            workspace_bucket: workspace_bucket.map(|b| b.into()),
        })
    }

    pub fn call_function<T: ToString>(self, template_address: TemplateAddress, function: T, args: Vec<Arg>) -> Self {
        self.add_instruction(Instruction::CallFunction {
            template_address,
            function: function.to_string(),
            args,
        })
    }

    pub fn call_method(self, component_address: ComponentAddress, method: &str, args: Vec<Arg>) -> Self {
        self.add_instruction(Instruction::CallMethod {
            component_address,
            method: method.to_string(),
            args,
        })
    }

    pub fn drop_all_proofs_in_workspace(self) -> Self {
        self.add_instruction(Instruction::DropAllProofsInWorkspace)
    }

    pub fn put_last_instruction_output_on_workspace<T: AsRef<[u8]>>(self, label: T) -> Self {
        self.add_instruction(Instruction::PutLastInstructionOutputOnWorkspace {
            key: label.as_ref().to_vec(),
        })
    }

    pub fn assert_bucket_contains<T: AsRef<[u8]>>(
        self,
        label: T,
        resource_address: ResourceAddress,
        min_amount: Amount,
    ) -> Self {
        self.add_instruction(Instruction::AssertBucketContains {
            key: label.as_ref().to_vec(),
            resource_address,
            min_amount,
        })
    }

    /// Publishing a WASM template.
    pub fn publish_template(self, binary: Vec<u8>) -> Self {
        self.add_instruction(Instruction::PublishTemplate { binary })
    }

    pub fn claim_burn(self, claim: ConfidentialClaim) -> Self {
        self.add_instruction(Instruction::ClaimBurn { claim: Box::new(claim) })
    }

    pub fn claim_validator_fees(self, address: ValidatorFeePoolAddress) -> Self {
        self.add_instruction(Instruction::ClaimValidatorFees { address })
    }

    pub fn create_proof(self, account: ComponentAddress, resource_addr: ResourceAddress) -> Self {
        // We may want to make this a native instruction
        self.add_instruction(Instruction::CallMethod {
            component_address: account,
            method: "create_proof_for_resource".to_string(),
            args: args![resource_addr],
        })
    }

    pub fn with_fee_instructions<I: IntoIterator<Item = Instruction>>(mut self, instructions: I) -> Self {
        self.unsigned_transaction.fee_instructions_mut().extend(instructions);
        // Reset the signatures as they are no longer valid
        self.clear_signatures();
        self
    }

    pub fn with_fee_instructions_builder<F: FnOnce(TransactionBuilder) -> TransactionBuilder>(mut self, f: F) -> Self {
        let builder = f(TransactionBuilder::new());
        *self.unsigned_transaction.fee_instructions_mut() = builder.unsigned_transaction.into_instructions();
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
        self.unsigned_transaction.inputs_mut().extend(inputs);
        // Reset the signatures as they are no longer valid
        self.clear_signatures();
        self
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

    pub fn build_unsigned_transaction(self) -> UnsignedTransaction {
        self.unsigned_transaction
    }

    pub fn add_signature(mut self, sealed_signer: &PublicKey, secret_key: &PrivateKey) -> Self {
        let signature = match &self.unsigned_transaction {
            UnsignedTransaction::V1(tx) => TransactionSignature::sign_v1(secret_key, sealed_signer, tx),
        };
        self.signatures.push(signature);
        self
    }

    fn clear_signatures(&mut self) {
        self.signatures = vec![];
    }

    pub fn build(self) -> UnsealedTransactionV1 {
        // Obviously this will not work if we have more than one version - dealing with that is left for another time
        match self.unsigned_transaction {
            UnsignedTransaction::V1(tx) => UnsealedTransactionV1::new(tx, self.signatures),
        }
    }

    pub fn build_and_seal(self, secret_key: &PrivateKey) -> Transaction {
        self.then(|builder| {
            // This is so that we dont have to add this in a lot of places - TODO: this is an assumption that may not
            // apply to all transactions
            if builder.signatures.is_empty() {
                builder.with_authorized_seal_signer()
            } else {
                builder
            }
        })
        .build()
        .seal(secret_key)
    }
}
