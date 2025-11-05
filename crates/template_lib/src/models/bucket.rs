//  Copyright 2022. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use serde::{Deserialize, Serialize};
use tari_bor::BorTag;
use tari_template_abi::{call_engine, rust::fmt, EngineOp};

use super::{
    BinaryTag,
    ConfidentialWithdrawProof,
    NonFungible,
    NonFungibleId,
    Proof,
    ResourceAddress,
    StealthTransferStatement,
};
use crate::{
    args::{BucketAction, BucketInvokeArg, BucketRef, InvokeResult},
    resource::ResourceManager,
    types::{Amount, ResourceType},
};

const TAG: u64 = BinaryTag::BucketId.as_u64();

/// A bucket identifier. This identifier is assigned at runtime.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Ord, PartialOrd, Hash)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct BucketId(BorTag<u32, TAG>);

impl From<u32> for BucketId {
    fn from(value: u32) -> Self {
        Self(BorTag::new(value))
    }
}

impl fmt::Display for BucketId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BucketId({})", self.0.inner())
    }
}

/// A temporary container of resources. Buckets exist during a transaction execution. All buckets must be
/// consumed (deposited into a vault, burned etc) before the end of the transaction or the entire transaction will fail.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Bucket {
    id: BucketId,
}

impl Bucket {
    /// Returns the BucketId of this bucket
    pub(crate) fn id(&self) -> BucketId {
        self.id
    }

    /// Returns the resource address of the tokens held in this bucket
    pub fn resource_address(&self) -> ResourceAddress {
        let resp: InvokeResult = call_engine(EngineOp::BucketInvoke, &BucketInvokeArg {
            bucket_ref: BucketRef::Ref(self.id),
            action: BucketAction::GetResourceAddress,
            args: invoke_args![],
        });

        resp.decode()
            .expect("Bucket GetResourceAddress returned invalid resource address")
    }

    /// Returns the type of resource held in this bucket
    pub fn resource_type(&self) -> ResourceType {
        let resp: InvokeResult = call_engine(EngineOp::BucketInvoke, &BucketInvokeArg {
            bucket_ref: BucketRef::Ref(self.id),
            action: BucketAction::GetResourceType,
            args: invoke_args![],
        });

        resp.decode()
            .expect("Bucket GetResourceType returned invalid resource type")
    }

    /// Withdraws `amount` tokens from the bucket into a new bucket.
    /// It will panic if there are not enough tokens in the bucket
    ///
    /// * for fungible resources, the `amount` is the number of tokens to withdraw
    /// * for non-fungible resources, the `amount` is the number of _non-specific_ non-fungible tokens to withdraw.
    ///   Prefer take_non_fungible if you want to withdraw specific non-fungible tokens.
    /// * for confidential resources, the `amount` is the number of revealed tokens to withdraw. Use `take_confidential`
    ///   to withdraw confidential tokens with a proof.
    pub fn take(&mut self, amount: Amount) -> Self {
        assert!(amount.is_positive());
        let resp: InvokeResult = call_engine(EngineOp::BucketInvoke, &BucketInvokeArg {
            bucket_ref: BucketRef::Ref(self.id),
            action: BucketAction::Take,
            args: invoke_args![amount],
        });

        resp.decode().expect("Bucket Take returned invalid bucket")
    }

    pub fn stealth_transfer(mut self, statement: StealthTransferStatement) -> Self {
        let manager = ResourceManager::get(self.resource_address());
        let output_bucket = if statement.inputs_statement.revealed_amount.is_positive() {
            let revealed_input_funds = self.take(statement.inputs_statement.revealed_amount);
            manager.stealth_transfer_with_opt_input_bucket(statement, Some(revealed_input_funds))
        } else {
            manager.stealth_transfer(statement)
        };

        if let Some(output_bucket) = output_bucket {
            // Put the output bucket back into the original bucket
            return self.join(output_bucket);
        }

        self
    }

    /// Takes (withdraws) confidential resources from the bucket into a new bucket.
    /// It will panic if the withdraw fails for any reason, including if the proof withdraws from unknown inputs,
    /// withdraws more funds than are available or is otherwise invalid.
    pub fn take_confidential(&mut self, proof: ConfidentialWithdrawProof) -> Self {
        let resp: InvokeResult = call_engine(EngineOp::BucketInvoke, &BucketInvokeArg {
            bucket_ref: BucketRef::Ref(self.id),
            action: BucketAction::TakeConfidential,
            args: invoke_args![proof],
        });

        resp.decode().expect("Bucket Take returned invalid bucket")
    }

    /// Destroy all the tokens that this bucket holds.
    /// It will panic if the caller does not have the appropriate permission to burn the resource.
    pub fn burn(&self) {
        let resp: InvokeResult = call_engine(EngineOp::BucketInvoke, &BucketInvokeArg {
            bucket_ref: BucketRef::Ref(self.id),
            action: BucketAction::Burn,
            args: invoke_args![],
        });

        resp.decode().expect("Bucket Burn returned invalid result")
    }

    /// Joins the bucket with another of the same resource, returning a new joined bucket with the value of both.
    /// Will panic if the other bucket does not contain the same resource.
    pub fn join(self, other: Bucket) -> Self {
        let resp: InvokeResult = call_engine(EngineOp::BucketInvoke, &BucketInvokeArg {
            bucket_ref: BucketRef::Ref(self.id),
            action: BucketAction::Join,
            args: invoke_args![other.id],
        });

        resp.decode().expect("Bucket join returned invalid result")
    }

    /// Drops the bucket if it is empty. Panics if the bucket is not empty.
    /// This must be called if all funds have been taken out of the bucket to prevent a dangling bucket error.
    pub fn drop_empty(self) {
        let resp: InvokeResult = call_engine(EngineOp::BucketInvoke, &BucketInvokeArg {
            bucket_ref: BucketRef::Ref(self.id),
            action: BucketAction::DropEmpty,
            args: invoke_args![],
        });

        resp.decode().expect("Bucket DropEmpty returned invalid result")
    }

    /// Returns true if the bucket is empty (i.e. contains zero tokens), otherwise false.
    pub fn is_empty(&self) -> bool {
        self.amount().is_zero()
    }

    /// Returns the amount of tokens held in this bucket.
    /// This includes any funds that are locked by a proof.
    ///
    /// Note that if the resource is confidential, only the revealed amount is returned.
    pub fn amount(&self) -> Amount {
        let resp: InvokeResult = call_engine(EngineOp::BucketInvoke, &BucketInvokeArg {
            bucket_ref: BucketRef::Ref(self.id),
            action: BucketAction::GetAmount,
            args: invoke_args![],
        });

        resp.decode().expect("Bucket GetAmount returned invalid amount")
    }

    /// Create a proof of ownership for all tokens in the bucket, used mainly for cross-template calls.
    /// Note that until the proof is dropped, the contained tokens are locked and cannot be used/deposited.
    pub fn create_proof(&self) -> Proof {
        let resp: InvokeResult = call_engine(EngineOp::BucketInvoke, &BucketInvokeArg {
            bucket_ref: BucketRef::Ref(self.id),
            action: BucketAction::CreateProof,
            args: invoke_args![],
        });

        resp.decode().expect("Bucket CreateProof returned invalid proof")
    }

    /// Returns the IDs of all the non-fungibles in this bucket
    /// If the resource is not a non-fungible resource, an empty vector is returned.
    pub fn get_non_fungible_ids(&self) -> Vec<NonFungibleId> {
        let resp: InvokeResult = call_engine(EngineOp::BucketInvoke, &BucketInvokeArg {
            bucket_ref: BucketRef::Ref(self.id),
            action: BucketAction::GetNonFungibleIds,
            args: invoke_args![],
        });

        resp.decode()
            .expect("get_non_fungible_ids returned invalid non fungible ids")
    }

    /// Returns all the non-fungibles in this bucket.
    /// If the resource is not a non-fungible resource, an empty vector is returned.
    pub fn get_non_fungibles(&self) -> Vec<NonFungible> {
        let resp: InvokeResult = call_engine(EngineOp::BucketInvoke, &BucketInvokeArg {
            bucket_ref: BucketRef::Ref(self.id),
            action: BucketAction::GetNonFungibles,
            args: invoke_args![],
        });

        resp.decode().expect("get_non_fungibles returned invalid non fungibles")
    }

    /// Returns the number of confidential commitments in this bucket.
    /// Note that this is not indicative of the number of tokens in the bucket, as these are blinded.
    pub fn count_confidential_commitments(&self) -> u32 {
        let resp: InvokeResult = call_engine(EngineOp::BucketInvoke, &BucketInvokeArg {
            bucket_ref: BucketRef::Ref(self.id),
            action: BucketAction::CountConfidentialCommitments,
            args: invoke_args![],
        });

        resp.decode()
            .expect("count_confidential_commitments returned invalid u32")
    }

    /// Asserts that the bucket does not contain any confidential commitments.
    /// This is useful to ensure that a bucket is not holding any confidential commitments before performing
    /// an operation e.g. depositing it into a vault that expects only revealed funds.
    pub fn assert_contains_no_confidential_funds(&self) {
        let count = self.count_confidential_commitments();
        assert_eq!(
            count, 0,
            "Expected bucket to have no confidential commitments, but found {count}",
        );
    }

    /// Creates a new bucket from a bucket ID. This is used internally. Performing any operations on this bucket will
    /// fail if the bucket is not in scope or does not exist. Rather use methods like `Vault::withdraw` to obtain a
    /// bucket.
    pub const fn from_id(id: BucketId) -> Self {
        Self { id }
    }
}

impl fmt::Display for Bucket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Bucket({})", self.id.0.inner())
    }
}
