//   Copyright 2022. The Tari Project
//
//   Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//   following conditions are met:
//
//   1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//   disclaimer.
//
//   2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//   following disclaimer in the documentation and/or other materials provided with the distribution.
//
//   3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//   products derived from this software without specific prior written permission.
//
//   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//   INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//   DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//   SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//   SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//   WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//   USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use tari_bor::BorError;
use tari_engine_types::{
    commit_result::RejectReason,
    entity_id_provider::EntityIdProviderError,
    id_provider::IdProviderError,
    indexed_value::IndexedValueError,
    lock::LockId,
    resource_container::ResourceError,
    substate::SubstateId,
    transaction_receipt::TransactionReceiptAddress,
    virtual_substate::VirtualSubstateId,
};
use tari_ootle_common_types::{displayable::Displayable, optional::IsNotFoundError};
use tari_template_lib::{
    args::VaultFreezeFlag,
    models::{
        AddressAllocationId,
        BucketId,
        ClaimedOutputTombstoneAddress,
        ComponentAddress,
        NonFungibleId,
        ProofId,
        ResourceAddress,
        VaultId,
    },
    types::{Amount, TemplateAddress},
};
use tari_transaction::args::{WorkspaceId, WorkspaceOffsetId};

use super::workspace::WorkspaceError;
use crate::{
    runtime::{locking::LockError, ActionIdent, RuntimeModuleError},
    state_store::StateStoreError,
};

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("Encoding error: {0}")]
    EncodingError(#[from] BorError),
    #[error("Indexed value error: {0}")]
    IndexedValueError(#[from] IndexedValueError),
    #[error("State storage error: {0}")]
    StateStoreError(#[from] StateStoreError),
    #[error("Workspace error: {0}")]
    WorkspaceError(#[from] WorkspaceError),
    #[error("Substate '{id}' not found or is not a transaction input")]
    SubstateNotFound { id: SubstateId },
    #[error("Root substate '{id}' not found")]
    RootSubstateNotFound { id: SubstateId },
    #[error("Referenced substate '{id}' not found")]
    ReferencedSubstateNotFound { id: SubstateId },
    #[error("Non-existent substate '{id}' returned from call")]
    NonExistentSubstateReturned { id: SubstateId },
    #[error("Substate '{id}' not in scope")]
    SubstateOutOfScope { id: SubstateId },
    #[error("Substate {id} is not owned by {requested_owner}")]
    SubstateNotOwned {
        id: SubstateId,
        // To reduce the size of this variant, we box one of the fields
        requested_owner: Box<SubstateId>,
    },
    #[error("Expected lock {lock_id} to lock {expected_type} but it locks {id}")]
    LockSubstateMismatch {
        lock_id: LockId,
        expected_type: &'static str,
        id: SubstateId,
    },
    #[error("Component {component} referenced an unknown substate {id}")]
    ComponentReferencedUnknownSubstate {
        component: ComponentAddress,
        id: SubstateId,
    },
    #[error("Encountered unknown or out of scope bucket {bucket_id}")]
    ValidationFailedBucketNotInScope { bucket_id: BucketId },
    #[error("Encountered unknown or out of scope proof {proof_id}")]
    ValidationFailedProofNotInScope { proof_id: ProofId },
    #[error("Encountered unknown or out of scope address allocation {id}")]
    ValidationFailedAddressAllocationNotInScope { id: AddressAllocationId },
    #[error("Component not found with address '{address}'")]
    ComponentNotFound { address: ComponentAddress },
    #[error("Layer one commitment not found with address '{address}'")]
    LayerOneCommitmentNotFound { address: ClaimedOutputTombstoneAddress },
    #[error("Invalid argument {argument}: {reason}")]
    InvalidArgument { argument: &'static str, reason: String },
    #[error("Invalid number of arguments: expected {expected}, but got {len}")]
    InvalidNumberOfArguments { expected: usize, len: usize },
    #[error("Invalid amount '{amount}': {reason}")]
    InvalidAmount { amount: Amount, reason: String },
    #[error("Call frame error: {details}")]
    CurrentFrameError { details: String },
    #[error("Vault not found with id ({vault_id})")]
    VaultNotFound { vault_id: VaultId },
    #[error("Vault {vault_id} is frozen for {freeze_flag}")]
    VaultFrozen {
        vault_id: VaultId,
        freeze_flag: VaultFreezeFlag,
    },
    #[error("Non-fungible token not found with address {resource_address} and id {nft_id}")]
    NonFungibleNotFound {
        resource_address: ResourceAddress,
        nft_id: NonFungibleId,
    },
    #[error("Invalid op '{op}' on burnt non-fungible {resource_address} id {nf_id}")]
    InvalidOpNonFungibleBurnt {
        op: &'static str,
        resource_address: ResourceAddress,
        nf_id: NonFungibleId,
    },
    #[error("Bucket not found with id {bucket_id}")]
    BucketNotFound { bucket_id: BucketId },
    #[error("Proof not found with id {proof_id}")]
    ProofNotFound { proof_id: ProofId },
    #[error("Resource not found with address {resource_address}")]
    ResourceNotFound { resource_address: ResourceAddress },
    #[error(transparent)]
    ResourceError(#[from] ResourceError),
    #[error(
        "Resource supply would overflow: resource {resource_address}, current supply {current_supply}, amount {amount}"
    )]
    ResourceSupplyWouldOverflow {
        resource_address: ResourceAddress,
        current_supply: Amount,
        amount: Amount,
    },
    #[error("Bucket {bucket_id} was dropped but was not empty")]
    BucketNotEmpty { bucket_id: BucketId },
    #[error("Item at id {id} does not exist on the workspace (existing ids: {})", .existing_ids.display())]
    ItemNotOnWorkspace {
        id: WorkspaceOffsetId,
        existing_ids: Vec<WorkspaceId>,
    },
    #[error("Attempted to take the last output but there was no previous instruction output")]
    NoLastInstructionOutput,
    #[error(transparent)]
    TransactionCommitError(#[from] TransactionCommitError),
    #[error("Transaction generated too many outputs: {0}")]
    TooManyOutputs(#[from] IdProviderError),
    #[error("Transaction generated too many new entities: {0}")]
    TooManyEntities(#[from] EntityIdProviderError),
    #[error("Duplicate NFT token id: {token_id}")]
    DuplicateNonFungibleId { token_id: NonFungibleId },
    #[error("Access Denied: {action_ident}")]
    AccessDenied { action_ident: ActionIdent },
    #[error("Access Denied: attempt to set state on component {attempted_on} from another component {attempted_by}")]
    AccessDeniedSetComponentState {
        attempted_on: SubstateId,
        // To reduce the size of this variant, we box one of the fields
        attempted_by: Box<SubstateId>,
    },
    #[error("Resource Auth Hook Denied Access for action {action_ident}: {details}")]
    AccessDeniedAuthHook { action_ident: ActionIdent, details: String },
    #[error("Access Denied: You must be the owner to perform this action: {action}")]
    AccessDeniedOwnerRequired { action: ActionIdent },
    #[error("Invalid method address rule for {template_name}: {details}")]
    InvalidMethodAccessRule { template_name: String, details: String },
    #[error("Runtime module error: {0}")]
    ModuleError(#[from] RuntimeModuleError),
    #[error("Invalid burn claim proof: {details}")]
    InvalidClaimProof { details: String },
    #[error("Layer one commitment already claimed with address '{address}'")]
    ConfidentialOutputAlreadyClaimed { address: ClaimedOutputTombstoneAddress },
    #[error("Template {template_address} not found")]
    TemplateNotFound { template_address: TemplateAddress },
    #[error("Insufficient fees paid: required {required_fee}, paid {fees_paid}")]
    InsufficientFeesPaid { required_fee: u64, fees_paid: u64 },
    #[error("No fees paid from stealth transfer: {details}")]
    NoFeesPaid { details: String },
    #[error("No fee checkpoint")]
    NoFeeCheckpoint,
    #[error("Component address must be sequential. Index before {index} was not found")]
    ComponentAddressMustBeSequential { index: u32 },
    #[error("Failed to load template '{address}': {details}")]
    FailedToLoadTemplate { address: TemplateAddress, details: String },
    #[error("Transaction Receipt already exists {address}")]
    TransactionReceiptAlreadyExists { address: TransactionReceiptAddress },
    #[error("Transaction Receipt not found")]
    TransactionReceiptNotFound,
    #[error("Component already exists {address}")]
    ComponentAlreadyExists { address: ComponentAddress },
    #[error("Cross-template call function error of function '{function}' on template '{template_address}': {details}")]
    CrossTemplateCallFunctionError {
        template_address: TemplateAddress,
        function: String,
        details: String,
    },
    #[error("Cross-template call failed for method '{method}' on component '{component_address}': {details}")]
    CrossTemplateCallMethodError {
        component_address: ComponentAddress,
        method: String,
        details: String,
    },
    #[error("Virtual substate not found: {address}")]
    VirtualSubstateNotFound { address: VirtualSubstateId },
    #[error("Invalid return value: {0}")]
    InvalidReturnValue(IndexedValueError),
    #[error("Attempt to pop auth scope stack but it was empty")]
    AuthScopeStackEmpty,
    #[error("Invalid deposit of bucket {bucket_id} has locked value amounting to {locked_amount}")]
    InvalidOpDepositLockedBucket { bucket_id: BucketId, locked_amount: Amount },
    #[error("Duplicate substate {address}")]
    DuplicateSubstate { address: SubstateId },
    #[error("Substate {address} is orphaned")]
    OrphanedSubstate { address: SubstateId },
    #[error("{} orphaned substate(s) detected: {}", .substates.len(), .substates.join(", "))]
    OrphanedSubstates { substates: Vec<String> },
    #[error("Attempted to finalise state but {remaining} call frame(s) remain on the stack")]
    CallFrameRemainingOnStack { remaining: usize },
    #[error("Duplicate reference to substate {address}")]
    DuplicateReference { address: SubstateId },
    #[error("Invalid argument: {0}")]
    ArgumentValidationError(#[from] ArgumentValidationError),
    #[error("Fee payment found in main intent, which is not allowed")]
    FeePaymentInMainIntent,

    #[error("BUG: [{function}] Invariant error {details}")]
    InvariantError { function: &'static str, details: String },
    #[error("Lock error: {0}")]
    LockError(#[from] LockError),
    #[error("{count} substate locks were still active after call")]
    DanglingSubstateLocks { count: usize },
    #[error("No active call frame")]
    NoActiveCallFrame,
    #[error("Max call depth {max_depth} exceeded")]
    MaxCallDepthExceeded { max_depth: usize },
    #[error("{action} can only be called from within a component context")]
    NotInComponentContext { action: ActionIdent },
    #[error("Duplicate bucket {bucket_id}")]
    DuplicateBucket { bucket_id: BucketId },
    #[error("Duplicate proof {proof_id}")]
    DuplicateProof { proof_id: ProofId },

    #[error("Address allocation not found with id {id}")]
    AddressAllocationNotFound { id: u32 },
    #[error("Address allocation with id {id} was not used")]
    AddressAllocationNotUsed { id: u32 },
    #[error("Address allocation type mismatch: got {id}, expected: {expected}")]
    AddressAllocationTypeMismatch { id: SubstateId, expected: &'static str },
    #[error("Allocated address does not have an associated template")]
    AddressAllocationNoTemplate,

    #[error("Invalid event topic '{topic}': {reason}")]
    InvalidEventTopic { topic: String, reason: String },

    #[error("Numeric conversion error: {details}")]
    NumericConversionError { details: String },
    #[error("Auth callback MUST return null, but it returned non-null")]
    UnexpectedNonNullInAuthHookReturn,

    #[error("Not supported: {details}")]
    NotSupported { details: String },

    #[error("Assert error: {0}")]
    AssertError(#[from] AssertError),

    #[error("Limit error: {details}")]
    LimitError { details: String },
}

impl RuntimeError {
    pub fn to_reject_reason(&self) -> RejectReason {
        match self {
            Self::SubstateNotFound { id } => RejectReason::OneOrMoreSubstatesNotFound(format!("{id} not found",)),
            Self::RootSubstateNotFound { id } => RejectReason::OneOrMoreSubstatesNotFound(format!(
                "Template referenced root substate but it was not found: {id}"
            )),
            Self::ReferencedSubstateNotFound { id } => RejectReason::OneOrMoreSubstatesNotFound(format!(
                "Template referenced substate but it was not found: {id}"
            )),
            Self::InsufficientFeesPaid {
                fees_paid,
                required_fee,
            } => RejectReason::InsufficientFeesPaid(format!(
                "Insufficient fees paid: {fees_paid}, required fees: {required_fee}"
            )),
            Self::FeePaymentInMainIntent => RejectReason::FeePaymentInMainIntent,
            err => RejectReason::ExecutionFailure(err.to_string()),
        }
    }
}

impl IsNotFoundError for RuntimeError {
    fn is_not_found_error(&self) -> bool {
        matches!(
            self,
            RuntimeError::SubstateNotFound { .. } |
                RuntimeError::ComponentNotFound { .. } |
                RuntimeError::VaultNotFound { .. } |
                RuntimeError::BucketNotFound { .. } |
                RuntimeError::ResourceNotFound { .. } |
                RuntimeError::NonFungibleNotFound { .. } |
                RuntimeError::ProofNotFound { .. }
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AssertError {
    #[error("The workspace value is not a bucket")]
    InvalidBucket,
    #[error("Assert expected bucket to have resource {expected} but has {got}")]
    InvalidResource {
        expected: ResourceAddress,
        got: ResourceAddress,
    },
    #[error("Assert expected bucket to have at least {expected} tokens but only has {got}")]
    InvalidAmount { expected: Amount, got: Amount },
}

#[derive(Debug, thiserror::Error)]
pub enum TransactionCommitError {
    #[error("{count} dangling buckets remain after transaction execution")]
    DanglingBuckets { count: usize },
    #[error("{count} dangling proofs remain after transaction execution")]
    DanglingProofs { count: usize },
    #[error("Locked value (amount: {locked_amount}) remaining in vault {vault_id}")]
    DanglingLockedValueInVault { vault_id: VaultId, locked_amount: Amount },
    #[error("{count} dangling address allocations remain after transaction execution")]
    DanglingAddressAllocations { count: usize },
    #[error("{count} dangling items in workspace after transaction execution")]
    WorkspaceNotEmpty { count: usize },
    #[error(transparent)]
    StateStoreError(#[from] StateStoreError),
    #[error(transparent)]
    IdProviderError(#[from] IdProviderError),
}

#[derive(Debug, thiserror::Error)]
pub enum ArgumentValidationError {
    #[error("Maximum number of stealth outputs exceeded: max {max_outputs}, got {actual_outputs}")]
    MaxStealthOutputsExceeded { max_outputs: usize, actual_outputs: usize },
    #[error("Maximum number of stealth inputs exceeded: max {max_inputs}, got {actual_inputs}")]
    MaxStealthInputsExceeded { max_inputs: usize, actual_inputs: usize },
    #[error("Too many arguments provided. Got {got}, max is {max}")]
    TooManyArguments { got: usize, max: usize },
}
