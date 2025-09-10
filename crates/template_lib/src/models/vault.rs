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
use tari_template_abi::{
    call_engine,
    rust::{
        collections::BTreeSet,
        fmt,
        fmt::{Display, Formatter},
        str::FromStr,
    },
    EngineOp,
};
use tari_template_lib_types::{EntityId, KeyParseError, ObjectKey};

use super::{
    address_prefixes,
    BinaryTag,
    Bucket,
    ConfidentialWithdrawProof,
    NonFungible,
    NonFungibleId,
    Proof,
    ProofAuth,
    ResourceAddress,
    StealthTransferStatement,
};
use crate::{
    args::{
        InvokeResult,
        PayFeeArg,
        VaultAction,
        VaultCreateProofByFungibleAmountArg,
        VaultCreateProofByNonFungiblesArg,
        VaultInvokeArg,
        VaultWithdrawArg,
    },
    newtype_struct_serde_impl,
    prelude::ResourceType,
    resource::ResourceManager,
    types::Amount,
};

const TAG: u64 = BinaryTag::VaultId as u64;

/// A vault's unique identification in the Tari network
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct VaultId(#[cfg_attr(feature = "ts", ts(type = "string"))] BorTag<ObjectKey, TAG>);

impl VaultId {
    pub const fn new(key: ObjectKey) -> Self {
        Self(BorTag::new(key))
    }

    pub fn from_hex(hex: &str) -> Result<Self, KeyParseError> {
        let key = ObjectKey::from_hex(hex)?;
        Ok(Self::new(key))
    }

    pub fn as_object_key(&self) -> &ObjectKey {
        self.0.inner()
    }

    pub fn entity_id(&self) -> EntityId {
        self.0.inner().as_entity_id()
    }
}

impl From<ObjectKey> for VaultId {
    fn from(key: ObjectKey) -> Self {
        Self::new(key)
    }
}

impl Display for VaultId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}_{}", address_prefixes::VAULT, *self.0)
    }
}

impl AsRef<[u8]> for VaultId {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl FromStr for VaultId {
    type Err = KeyParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.strip_prefix("vault_").unwrap_or(s);
        Self::from_hex(s)
    }
}

impl TryFrom<&[u8]> for VaultId {
    type Error = KeyParseError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        let key = ObjectKey::try_from(value)?;
        Ok(Self::new(key))
    }
}

newtype_struct_serde_impl!(VaultId, BorTag<ObjectKey, TAG>);

#[cfg(feature = "borsh")]
mod borsh_impl {
    use std::io::Read;

    use super::*;
    impl ::borsh::BorshSerialize for VaultId {
        fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
            borsh::BorshSerialize::serialize(self.as_object_key().array(), writer)
        }
    }

    impl borsh::BorshDeserialize for VaultId {
        fn deserialize_reader<R: Read>(reader: &mut R) -> std::io::Result<Self> {
            let key = borsh::BorshDeserialize::deserialize_reader(reader)?;
            Ok(Self::new(ObjectKey::from_array(key)))
        }
    }
}

/// Encapsulates all the ways that a vault can be referenced
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum VaultRef {
    Vault { address: ResourceAddress },
    Ref(VaultId),
}

impl VaultRef {
    pub fn resource_address(&self) -> Option<&ResourceAddress> {
        match self {
            VaultRef::Vault { address, .. } => Some(address),
            VaultRef::Ref(_) => None,
        }
    }

    pub fn vault_id(&self) -> Option<VaultId> {
        match self {
            VaultRef::Vault { .. } => None,
            VaultRef::Ref(id) => Some(*id),
        }
    }
}

impl Display for VaultRef {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            VaultRef::Vault { address, .. } => write!(f, "Vaults({})", address),
            VaultRef::Ref(id) => write!(f, "Ref({})", id),
        }
    }
}

/// References a secure container of a single resource and provides an abstraction for vault operations.
/// A vault can hold fungible tokens, non-fungible tokens, or confidential tokens.
/// A vault is identified by a globally-unique `VaultId`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Vault {
    vault_id: VaultId,
}

impl Vault {
    /// Creates a new empty vault for the provided resource address.
    pub fn new_empty(resource_address: ResourceAddress) -> Self {
        let resp: InvokeResult = call_engine(EngineOp::VaultInvoke, &VaultInvokeArg {
            vault_ref: VaultRef::Vault {
                address: resource_address,
            },
            action: VaultAction::Create,
            args: invoke_args![],
        });

        Self {
            vault_id: resp.decode().unwrap(),
        }
    }

    /// Returns a new vault that contains all the tokens from the provided bucket.
    /// The bucket is consumed and cannot be used after this call.
    ///
    /// # Panics
    /// if the the bucket contains a different resource than the vault
    ///
    /// # Example
    /// ```rust,ignore
    /// use tari_template_lib::models::{Bucket, Vault};
    /// let bucket = self.get_a_bucket();
    /// let vault = Vault::from_bucket(bucket);
    /// ```
    pub fn from_bucket(bucket: Bucket) -> Self {
        let resource_address = bucket.resource_address();
        let vault = Self::new_empty(resource_address);
        vault.deposit(bucket);
        vault
    }

    /// Deposit all the tokens from the provided bucket into the vault.
    /// The bucket is consumed and cannot be used after this call.
    ///
    /// # Panics
    /// If the the bucket contains a different resource than the vault
    pub fn deposit(&self, bucket: Bucket) {
        let result: InvokeResult = call_engine(EngineOp::VaultInvoke, &VaultInvokeArg {
            vault_ref: self.vault_ref(),
            action: VaultAction::Deposit,
            args: invoke_args![bucket.id()],
        });

        result.decode::<()>().expect("deposit failed");
    }

    /// Withdraw an `amount` of tokens from the vault and creates a new bucket to contain them.
    ///
    /// * For fungible resources, withdraw `amount` tokens.
    /// * For non-fungible resources, withdraw a `amount` non-fungible tokens in an unspecified order. Typically,
    ///   `withdraw_non_fungible` is more useful for non-fungible resources.
    /// * For confidential resources, this call panics and the transaction fails.
    pub fn withdraw<T: Into<Amount>>(&self, amount: T) -> Bucket {
        let resp: InvokeResult = call_engine(EngineOp::VaultInvoke, &VaultInvokeArg {
            vault_ref: self.vault_ref(),
            action: VaultAction::Withdraw,
            args: invoke_args![VaultWithdrawArg::Fungible { amount: amount.into() }],
        });

        resp.decode().expect("failed to decode Bucket")
    }

    /// Withdraw a single non-fungible token from the vault into a new bucket.
    /// It will panic if the vault does not contain the specified non-fungible token
    pub fn withdraw_non_fungible(&self, id: NonFungibleId) -> Bucket {
        self.withdraw_non_fungibles(Some(id))
    }

    /// Withdraw multiple non-fungible tokens from the vault into a new bucket.
    /// It will panic if the vault does not contain the specified non-fungible tokens
    pub fn withdraw_non_fungibles<I: IntoIterator<Item = NonFungibleId>>(&self, ids: I) -> Bucket {
        let resp: InvokeResult = call_engine(EngineOp::VaultInvoke, &VaultInvokeArg {
            vault_ref: self.vault_ref(),
            action: VaultAction::Withdraw,
            args: invoke_args![VaultWithdrawArg::NonFungible {
                ids: ids.into_iter().collect()
            }],
        });

        resp.decode().expect("failed to decode Bucket")
    }

    /// Withdraws confidential resources from the vault into a new bucket.
    /// It will panic if the withdraw fails for any reason, including if the proof withdraws from unknown inputs,
    /// withdraws more funds than are available or is otherwise invalid.
    ///
    /// # Example
    /// ```rust,ignore
    /// use tari_template_lib::models::{Vault, ConfidentialWithdrawProof};
    /// let vault = self.my_vault;
    /// let proof = // .. wallet generates a ConfidentialWithdrawProof
    /// let bucket = vault.withdraw_confidential(proof);
    /// /// // The bucket now contains the withdrawn confidential funds
    /// ```
    pub fn withdraw_confidential(&self, proof: ConfidentialWithdrawProof) -> Bucket {
        let resp: InvokeResult = call_engine(EngineOp::VaultInvoke, &VaultInvokeArg {
            vault_ref: self.vault_ref(),
            action: VaultAction::Withdraw,
            args: invoke_args![VaultWithdrawArg::Confidential { proof: Box::new(proof) }],
        });

        resp.decode().expect("failed to decode Bucket")
    }

    /// Withdraws all fungible, non-fungible and revealed confidential amounts from the vault into a new bucket.
    /// NOTE: blinded confidential amounts are not withdrawn as these require a `ConfidentialWithdrawProof`.
    pub fn withdraw_all(&mut self) -> Bucket {
        self.withdraw(self.balance())
    }

    /// Returns how many tokens this vault holds.
    ///
    /// * For fungible resources, returns the total amount of tokens.
    /// * For non-fungible resources, returns the total number of non-fungible tokens.
    /// * For confidential resources, returns the total amount of revealed tokens. Confidential tokens are not included
    ///   in this amount as these are blinded.
    pub fn balance(&self) -> Amount {
        let resp: InvokeResult = call_engine(EngineOp::VaultInvoke, &VaultInvokeArg {
            vault_ref: self.vault_ref(),
            action: VaultAction::GetBalance,
            args: invoke_args![],
        });

        resp.decode().expect("failed to decode Amount")
    }

    /// Returns how many tokens are locked (unspendable) in this vault.
    pub fn locked_balance(&self) -> Amount {
        let resp: InvokeResult = call_engine(EngineOp::VaultInvoke, &VaultInvokeArg {
            vault_ref: self.vault_ref(),
            action: VaultAction::GetLockedBalance,
            args: invoke_args![],
        });

        resp.decode().expect("failed to decode Amount")
    }

    /// Returns how many confidential outputs this vault holds.
    /// This is not indicative of the value of the confidential tokens (these are blinded).
    pub fn commitment_count(&self) -> u32 {
        let resp: InvokeResult = call_engine(EngineOp::VaultInvoke, &VaultInvokeArg {
            vault_ref: self.vault_ref(),
            action: VaultAction::GetCommitmentCount,
            args: invoke_args![],
        });

        resp.decode().expect("failed to decode commitment count")
    }

    /// Returns the IDs of all the non-fungibles contained in this vault.
    pub fn get_non_fungible_ids(&self) -> Vec<NonFungibleId> {
        let resp: InvokeResult = call_engine(EngineOp::VaultInvoke, &VaultInvokeArg {
            vault_ref: self.vault_ref(),
            action: VaultAction::GetNonFungibleIds,
            args: invoke_args![],
        });

        resp.decode()
            .expect("get_non_fungible_ids returned invalid non fungible ids")
    }

    /// Returns all the non-fungibles in this vault including their metadata.
    pub fn get_non_fungibles(&self) -> Vec<NonFungible> {
        let resp: InvokeResult = call_engine(EngineOp::VaultInvoke, &VaultInvokeArg {
            vault_ref: self.vault_ref(),
            action: VaultAction::GetNonFungibles,
            args: invoke_args![],
        });

        resp.decode().expect("get_non_fungibles returned invalid non fungibles")
    }

    /// Returns the resource address of the tokens that this vault holds
    pub fn resource_address(&self) -> ResourceAddress {
        let resp: InvokeResult = call_engine(EngineOp::VaultInvoke, &VaultInvokeArg {
            vault_ref: self.vault_ref(),
            action: VaultAction::GetResourceAddress,
            args: invoke_args![],
        });

        resp.decode()
            .expect("GetResourceAddress returned invalid resource address")
    }

    /// Returns the the type of resource that this vault holds.
    pub fn resource_type(&self) -> ResourceType {
        ResourceManager::get(self.resource_address()).resource_type()
    }

    /// Pay a transaction fee with revealed funds present in the vault.
    /// Note that the vault must hold native Tari tokens to perform this operation.
    pub fn pay_fee<A: Into<Amount>>(&self, amount: A) {
        let _resp: InvokeResult = call_engine(EngineOp::VaultInvoke, &VaultInvokeArg {
            vault_ref: self.vault_ref(),
            action: VaultAction::PayFee,
            args: invoke_args![PayFeeArg {
                amount: amount.into(),
                statement: None
            }],
        });
    }

    /// Pay a transaction fee with stealth funds.
    /// Note that the vault must hold native XTR tokens to perform this operation
    /// The transfer statement must result in a positive amount of revealed funds (from this vault and/or from consumed
    /// utxos).
    pub fn pay_fee_stealth(&self, transfer: StealthTransferStatement) {
        let _resp: InvokeResult = call_engine(EngineOp::VaultInvoke, &VaultInvokeArg {
            vault_ref: self.vault_ref(),
            action: VaultAction::PayFee,
            args: invoke_args![PayFeeArg {
                amount: Amount::zero(),
                statement: Some(transfer)
            }],
        });
    }

    /// Creates a new [ProofAuth] that references a proof that proves ownership of all the vault's tokens.
    ///
    /// # Example
    /// ```rust,ignore
    /// use tari_template_lib::models::Vault;
    /// let vault = self.my_vault;
    /// let _auth = vault.authorize(); // RAII pattern
    /// perform_some_action_that_requires_authorization();
    /// // _auth is dropped here, and the proof is goes out of scope
    /// /// ```
    pub fn authorize(&self) -> ProofAuth {
        let proof = self.create_proof();
        ProofAuth { id: proof.id() }
    }

    /// Uses all the tokens in the vault to authorize some actions within the provided function.
    /// All tokens will be locked during the lifespan of the transaction until the proof is destroyed after the function
    /// returns.
    ///
    /// # Example
    /// ```rust,ignore
    /// use tari_template_lib::models::Vault;
    /// let vault = self.my_vault;
    /// vault.authorize_with(|| {
    ///     perform_some_action_that_requires_authorization();
    /// });
    pub fn authorize_with<F: FnOnce() -> R, R>(&self, f: F) -> R {
        let _auth = self.authorize();
        f()
    }

    /// Returns a new proof that demonstrates ownership of all the vault's tokens.
    /// The tokens will be locked during the lifespan of the transaction until the proof is destroyed.
    /// Used mostly for cross-component calls
    pub fn create_proof(&self) -> Proof {
        let resp: InvokeResult = call_engine(EngineOp::VaultInvoke, &VaultInvokeArg {
            vault_ref: self.vault_ref(),
            action: VaultAction::CreateProofByResource,
            args: invoke_args![],
        });

        resp.decode().expect("CreateProofOfResource failed")
    }

    /// Returns a new proof that demonstrates ownership of a specific amount of tokens.
    /// The tokens will be locked during the lifespan of the proof i.e until proof.drop() is called.
    /// Used primarily to authorize cross-component calls.
    pub fn create_proof_by_amount<A: Into<Amount>>(&self, amount: A) -> Proof {
        let resp: InvokeResult = call_engine(EngineOp::VaultInvoke, &VaultInvokeArg {
            vault_ref: self.vault_ref(),
            action: VaultAction::CreateProofByFungibleAmount,
            args: invoke_args![VaultCreateProofByFungibleAmountArg { amount: amount.into() }],
        });

        resp.decode().expect("CreateProofByFungibleAmount failed")
    }

    /// Returns a new proof that demonstrates ownership of a specific set of non-fungibles.
    /// The tokens will be locked during the lifespan of the proof i.e until proof.drop() is called.
    /// Used primarily to authorize cross-component calls using "badges".
    pub fn create_proof_by_non_fungible_ids(&self, ids: BTreeSet<NonFungibleId>) -> Proof {
        let resp: InvokeResult = call_engine(EngineOp::VaultInvoke, &VaultInvokeArg {
            vault_ref: self.vault_ref(),
            action: VaultAction::CreateProofByNonFungibles,
            args: invoke_args![VaultCreateProofByNonFungiblesArg { ids }],
        });

        resp.decode().expect("CreateProofByNonFungibles failed")
    }

    /// Returns the VaultId of this vault.
    pub fn vault_id(&self) -> VaultId {
        self.vault_id
    }

    fn vault_ref(&self) -> VaultRef {
        VaultRef::Ref(self.vault_id)
    }

    /// Creates a vault from the provided `VaultId`. Unless the transaction is authorized to use this vault, performing
    /// any operations on the resulting vault will fail. This is used in unit tests to test various "unusual" scenarios
    /// and should not be used in production templates.
    pub fn for_test(vault_id: VaultId) -> Self {
        Self { vault_id }
    }
}
