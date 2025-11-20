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

use std::{collections::HashMap, time::Duration};

use serde::{Deserialize, Serialize};
use tari_engine_types::{
    commit_result::{ExecuteResult, FinalizeResult},
    confidential::MinotariBurnClaimProof,
    serde_with,
    substate::{Substate, SubstateId},
    ValidatorFeePoolAddress,
};
use tari_ootle_address::OotleAddress;
use tari_ootle_common_types::{
    shard::Shard,
    substate_type::SubstateType,
    ShardGroup,
    SubstateAddress,
    SubstateRequirement,
};
use tari_ootle_wallet_sdk::{
    apis::{
        confidential_transfer::UtxoInputSelection,
        stealth_transfer::{BadgeUsage, PayTo, TransferOutput},
    },
    crypto::memo::Memo,
    models::{
        Account,
        AuthoredTemplateModel,
        DerivedKeyIndex,
        KeyBranch,
        KeyId,
        NonFungibleToken,
        OutputStatus,
        StealthUtxoSpendKeyId,
        TransactionStatus,
        WalletLockId,
        WalletTransaction,
    },
};
use tari_template_abi::{FunctionDef, TemplateDef};
use tari_template_lib::{
    models::{
        ConfidentialOutputStatement,
        NonFungibleId,
        ResourceAddress,
        SpendCondition,
        StealthTransferStatement,
        UtxoAddress,
        UtxoId,
        VaultId,
    },
    prelude::{ComponentAddress, ConfidentialWithdrawProof, ResourceType, RistrettoPublicKeyBytes},
    types::{crypto::PedersenCommitmentBytes, Amount, EncryptedData, TemplateAddress},
};
use tari_transaction::{Instruction, Transaction, TransactionId, UnsignedTransaction};
use time::PrimitiveDateTime;
use webauthn_rs_proto::{
    PublicKeyCredential,
    PublicKeyCredentialCreationOptions,
    RegisterPublicKeyCredential,
    RequestChallengeResponse,
};
use zeroize::Zeroizing;

use crate::{
    permissions::Claims,
    serialize::{opt_string_or_struct, string_or_struct},
    ComponentAddressOrName,
};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct CallInstructionRequest {
    pub instructions: Vec<Instruction>,
    #[serde(deserialize_with = "string_or_struct")]
    pub fee_account: ComponentAddressOrName,
    #[serde(default, deserialize_with = "opt_string_or_struct")]
    pub dump_outputs_into: Option<ComponentAddressOrName>,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub max_fee: u64,
    #[serde(default)]
    pub inputs: Vec<SubstateRequirement>,
    #[serde(default)]
    pub override_inputs: Option<bool>,
    #[serde(default)]
    pub new_outputs: Option<u8>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(type = "Array<number>"))]
    pub proof_ids: Vec<WalletLockId>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub min_epoch: Option<u64>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub max_epoch: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct TransactionSubmitRequest {
    pub transaction: UnsignedTransaction,
    pub seal_signer: KeyId,
    pub other_signers: Vec<KeyId>,
    /// Attempt to infer inputs and their dependencies from instructions. If false, the provided transaction must
    /// contain the required inputs.
    pub detect_inputs: bool,
    /// If true(default), detected inputs will omit versions allowing consensus to resolve input substates.
    /// If false, the wallet will try to determine versions for the inputs. These may be outdated if the substate has
    /// changed since detection.
    #[serde(default = "return_true")]
    pub detect_inputs_use_unversioned: bool,
    pub lock_ids: Vec<WalletLockId>,
}

const fn return_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct TransactionSubmitResponse {
    pub transaction_id: TransactionId,
}

pub type TransactionSubmitDryRunRequest = TransactionSubmitRequest;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct TransactionSubmitDryRunResponse {
    pub transaction_id: TransactionId,
    pub result: ExecuteResult,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct TransactionSubmitManifestRequest {
    pub manifest: String,
    pub variables: HashMap<String, String>,
    pub signing_key_id: Option<KeyId>,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub max_fee: u64,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct TransactionSubmitManifestResponse {
    pub transaction_id: TransactionId,
    pub result: Option<ExecuteResult>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct PublishTemplateRequest {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    #[serde(with = "serde_with::base64")]
    pub binary: Vec<u8>,
    #[serde(deserialize_with = "opt_string_or_struct")]
    pub fee_account: Option<ComponentAddressOrName>,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub max_fee: u64,
    /// Attempt to infer inputs and their dependencies from instructions. If false, the provided transaction must
    /// contain the required inputs.
    pub detect_inputs: bool,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct PublishTemplateResponse {
    pub transaction_id: TransactionId,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub dry_run_fee: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct TransactionGetRequest {
    pub transaction_id: TransactionId,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct TransactionGetResponse {
    pub transaction: Transaction,
    pub result: Option<FinalizeResult>,
    pub status: TransactionStatus,
    pub invalid_reason: Option<String>,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub last_update_time: PrimitiveDateTime,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct TransactionGetAllRequest {
    pub status: Option<TransactionStatus>,
    pub component: Option<ComponentAddress>,
    pub signer_public_key: Option<RistrettoPublicKeyBytes>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct TransactionGetAllResponse {
    pub transactions: Vec<WalletTransaction>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct TransactionGetResultRequest {
    pub transaction_id: TransactionId,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct TransactionGetResultResponse {
    pub transaction_id: TransactionId,
    pub status: TransactionStatus,
    pub result: Option<FinalizeResult>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct TransactionWaitResultRequest {
    pub transaction_id: TransactionId,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct TransactionWaitResultResponse {
    pub transaction_id: TransactionId,
    pub result: Option<FinalizeResult>,
    pub status: TransactionStatus,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub final_fee: u64,
    pub timed_out: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct TransactionClaimBurnResponse {
    pub transaction_id: TransactionId,
    pub inputs: Vec<SubstateAddress>,
    pub outputs: Vec<SubstateAddress>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct KeysListRequest {
    pub branch: KeyBranch,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct KeysListResponse {
    /// (KeyId, public key, is_active)
    pub keys: Vec<(KeyId, RistrettoPublicKeyBytes, bool)>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct KeysSetActiveRequest {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub index: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct KeysSetActiveResponse {
    pub public_key: RistrettoPublicKeyBytes,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct KeysCreateRequest {
    pub branch: KeyBranch,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub specific_index: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct KeysCreateResponse {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub id: u64,
    pub public_key: RistrettoPublicKeyBytes,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AccountsCreateRequest {
    pub account_name: Option<String>,
    pub is_default: Option<bool>,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub key_index: Option<DerivedKeyIndex>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AccountsCreateResponse {
    pub account: Account,
    pub address: OotleAddress,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AccountsCreateOrGetRequest {
    pub account: Option<ComponentAddressOrName>,
    pub is_default: Option<bool>,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub key_index: Option<DerivedKeyIndex>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AccountsCreateOrGetResponse {
    pub account: Account,
    pub address: OotleAddress,
    pub created: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AccountsListRequest {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub offset: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub limit: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AccountInfo {
    pub account: Account,
    pub address: OotleAddress,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AccountsListResponse {
    pub accounts: Vec<AccountInfo>,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub total: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AccountsGetBalancesRequest {
    #[serde(deserialize_with = "opt_string_or_struct")]
    pub account: Option<ComponentAddressOrName>,
    #[serde(default)]
    pub refresh: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AccountsGetBalancesResponse {
    pub address: ComponentAddress,
    pub balances: Vec<BalanceEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct BalanceEntry {
    pub vault_address: Option<VaultId>,
    pub resource_address: ResourceAddress,
    pub balance: Amount,
    pub resource_type: ResourceType,
    pub confidential_balance: Amount,
    pub token_symbol: Option<String>,
    pub divisibility: u8,
}

impl BalanceEntry {
    pub fn to_balance_string(&self) -> String {
        let symbol = self.token_symbol.as_deref().unwrap_or_default();
        match self.resource_type {
            ResourceType::Fungible => {
                format!("{} {}", self.balance, symbol)
            },
            ResourceType::NonFungible => {
                format!("{} {} tokens", self.balance, symbol)
            },
            ResourceType::Confidential => {
                format!(
                    "{} revealed + {} blinded = {} {}",
                    self.balance,
                    self.confidential_balance,
                    self.balance + self.confidential_balance,
                    symbol
                )
            },
            ResourceType::Stealth => {
                format!("{} {} (stealth)", self.balance + self.confidential_balance, symbol)
            },
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AccountGetRequest {
    #[serde(deserialize_with = "string_or_struct")]
    pub name_or_address: ComponentAddressOrName,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AccountGetDefaultRequest {
    // Intentionally empty. Fields may be added in the future.
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AccountGetByKeyIndexRequest {
    pub key_index: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AccountGetResponse {
    pub account: Account,
    pub address: OotleAddress,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AccountSetDefaultRequest {
    #[serde(deserialize_with = "string_or_struct")]
    pub account: ComponentAddressOrName,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AccountSetDefaultResponse {}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AccountsRenameRequest {
    #[serde(deserialize_with = "string_or_struct")]
    pub account: ComponentAddressOrName,
    pub new_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AccountsRenameResponse {}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AccountsTransferRequest {
    #[serde(deserialize_with = "opt_string_or_struct")]
    pub account: Option<ComponentAddressOrName>,
    pub amount: Amount,
    pub resource_address: ResourceAddress,
    pub destination_public_key: RistrettoPublicKeyBytes,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub max_fee: Option<u64>,
    pub proof_from_badge_resource: Option<ResourceAddress>,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AccountsTransferResponse {
    pub transaction_id: TransactionId,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct ProofsGenerateRequest {
    pub confidential_amount: Amount,
    pub reveal_amount: Amount,
    #[serde(deserialize_with = "opt_string_or_struct")]
    pub account: Option<ComponentAddressOrName>,
    pub resource_address: ResourceAddress,
    pub destination_public_key: RistrettoPublicKeyBytes,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memo: Option<Memo>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct ProofsGenerateResponse {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub proof_id: WalletLockId,
    pub proof: ConfidentialWithdrawProof,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct ProofsFinalizeRequest {
    pub lock_id: WalletLockId,
    pub transaction_id: TransactionId,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct ProofsFinalizeResponse {}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct ProofsCancelRequest {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub proof_id: WalletLockId,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct ConfidentialCreateOutputProofRequest {
    pub amount: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct ConfidentialCreateOutputProofResponse {
    pub proof: ConfidentialOutputStatement,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct ConfidentialTransferRequest {
    #[serde(deserialize_with = "opt_string_or_struct")]
    pub account: Option<ComponentAddressOrName>,
    pub amount: Amount,
    pub input_selection: UtxoInputSelection,
    pub resource_address: ResourceAddress,
    pub destination_address: OotleAddress,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub max_fee: Option<u64>,
    pub output_to_revealed: bool,
    pub proof_from_badge_resource: Option<ResourceAddress>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memo: Option<Memo>,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct ConfidentialTransferResponse {
    pub transaction_id: TransactionId,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct ConfidentialViewVaultBalanceRequest {
    pub vault_id: VaultId,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub minimum_expected_value: Option<u64>,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub maximum_expected_value: Option<u64>,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub view_key_id: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct ConfidentialViewVaultBalanceResponse {
    #[cfg_attr(feature = "ts", ts(type = "Record<string, number | null>"))]
    pub balances: HashMap<PedersenCommitmentBytes, Option<u64>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct ClaimBurnRequest {
    pub account: ComponentAddressOrName,
    pub claim_proof: ClaimBurnProof,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub max_fee: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct ClaimBurnProof {
    pub claim_proof: MinotariBurnClaimProof,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub owner_nonce_key_index: DerivedKeyIndex,
    pub encrypted_data: EncryptedData,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct ClaimBurnResponse {
    pub transaction_id: TransactionId,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct ProofsCancelResponse {}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AccountsCreateFreeTestCoinsRequest {
    pub account: ComponentAddressOrName,
    pub amount: Amount,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub max_fee: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AccountsCreateFreeTestCoinsResponse {
    pub account: Account,
    pub transaction_id: TransactionId,
    pub amount: Amount,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub fee: u64,
    pub result: FinalizeResult,
    pub address: OotleAddress,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct WebRtcStart {
    pub jwt: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct WebRtcStartRequest {
    pub signaling_server_token: String,
    #[cfg_attr(feature = "ts", ts(type = "object"))]
    pub permissions: serde_json::Value,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct WebRtcStartResponse {}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AuthLoginRequest {
    pub permissions: Vec<String>,
    #[cfg_attr(feature = "ts", ts(type = "{secs: number, nanos: number} | null"))]
    pub duration: Option<Duration>,
    pub webauthn_finish_auth_request: Option<WebauthnFinishAuthRequest>,
}

/// Represents a JWT token. The token is zeroized from memory on drop.
pub type EncodedJwtString = Zeroizing<String>;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AuthLoginResponse {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub auth_token: EncodedJwtString,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub valid_for_secs: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AuthLoginAcceptRequest {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub auth_token: EncodedJwtString,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AuthLoginAcceptResponse {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub permissions_token: EncodedJwtString,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AuthLoginDenyRequest {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub auth_token: EncodedJwtString,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AuthLoginDenyResponse {}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AuthRevokeTokenRequest {
    pub permission_token_id: i32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AuthRevokeTokenResponse {}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct MintFaucetNftRequest {
    pub account: ComponentAddressOrName,
    #[cfg_attr(feature = "ts", ts(type = "object"))]
    pub mutable_data: serde_json::Value,
    pub number_to_mint: u64,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub max_fee: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct MintFaucetNftResponse {
    pub transaction_id: TransactionId,
    pub finalize: FinalizeResult,
    pub fee: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct GetNftRequest {
    pub resource_address: ResourceAddress,
    pub nft_id: NonFungibleId,
}

pub type GetNftResponse = NonFungibleToken;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct ListNftsRequest {
    #[serde(deserialize_with = "opt_string_or_struct")]
    pub account: Option<ComponentAddressOrName>,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub limit: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub offset: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct ListNftsResponse {
    pub nfts: Vec<NonFungibleToken>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AuthGetAllJwtRequest {}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AuthGetAllJwtResponse {
    pub jwt: Vec<Claims>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct GetValidatorFeesRequest {
    pub account_or_key: AccountOrKeyId,
    pub shard_group: Option<ShardGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub enum AccountOrKeyId {
    /// Query by account. None signifies the default account.
    Account(Option<ComponentAddressOrName>),
    /// Query by key id.
    KeyId(KeyId),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct GetValidatorFeesResponse {
    pub fees: HashMap<Shard, FeePoolDetails>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct FeePoolDetails {
    pub address: ValidatorFeePoolAddress,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub amount: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct ClaimValidatorFeesRequest {
    #[serde(default, deserialize_with = "opt_string_or_struct")]
    pub account: Option<ComponentAddressOrName>,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub claim_key_index: Option<u64>,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub max_fee: Option<u64>,
    pub shards: Vec<Shard>,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct ClaimValidatorFeesResponse {
    pub transaction_id: TransactionId,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub fee: u64,
    pub result: FinalizeResult,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct SettingsSetRequest {
    pub indexer_url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct SettingsSetResponse {}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct SettingsGetResponse {
    pub indexer_url: String,
    pub network: NetworkInfo,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct NetworkInfo {
    pub name: String,
    pub byte: u8,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct SubstatesListRequest {
    #[serde(default, deserialize_with = "serde_with::string::option::deserialize")]
    #[cfg_attr(feature = "ts", ts(type = "string | null"))]
    pub filter_by_template: Option<TemplateAddress>,
    pub filter_by_type: Option<SubstateType>,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub limit: Option<u64>,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub offset: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct SubstatesListResponse {
    pub substates: Vec<WalletSubstateInfo>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct SubstatesGetRequest {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub substate_id: SubstateId,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct SubstatesGetResponse {
    // NOTE either of these can be None, but never both (instead, NotFound error)
    pub local_record: Option<WalletSubstateInfo>,
    pub substate_from_remote: Option<Substate>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct WalletSubstateInfo {
    pub substate_id: SubstateId,
    pub parent_id: Option<SubstateId>,
    pub module_name: Option<String>,
    pub version: u32,
    pub template_address: Option<TemplateAddress>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct TemplatesGetRequest {
    pub template_address: TemplateAddress,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct TemplatesGetResponse {
    pub template_definition: TemplateDef,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct TemplatesListAuthoredRequest {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub author_public_key: RistrettoPublicKeyBytes,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub page: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub page_size: u64,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthoredTemplate {
    pub author_public_key: RistrettoPublicKeyBytes,
    pub address: TemplateAddress,
    pub name: String,
    pub tari_version: String,
    pub functions: Vec<FunctionDef>,
}

impl From<&AuthoredTemplateModel> for AuthoredTemplate {
    fn from(model: &AuthoredTemplateModel) -> Self {
        AuthoredTemplate {
            author_public_key: model.author_public_key,
            address: model.address,
            name: model.name.clone(),
            tari_version: model.tari_version.clone(),
            functions: model.functions.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct TemplatesListAuthoredResponse {
    pub templates: Vec<AuthoredTemplate>,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub total_templates: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AuthGetMethodRequest {}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
#[serde(rename_all = "lowercase")]
pub enum AuthMethod {
    None,
    Webauthn,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AuthGetMethodResponse {
    pub method: AuthMethod,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct WebauthnAlreadyRegisteredRequest {
    pub username: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct WebauthnAlreadyRegisteredResponse {
    pub registered: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct WebauthnStartRegisterRequest {
    pub username: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct WebauthnStartRegisterResponse {
    /// Unique ID of the current registration Session.
    pub session_id: String,
    /// [`PublicKeyCredentialCreationOptions`] serialized as JSON
    #[cfg_attr(feature = "ts", ts(type = "object"))]
    pub public_key: PublicKeyCredentialCreationOptions,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct WebauthnFinishRegisterRequest {
    /// Session ID received from [`WebauthnStartRegisterResponse`].
    pub session_id: String,
    /// [`RegisterPublicKeyCredential`] serialized as JSON.
    #[cfg_attr(feature = "ts", ts(type = "object"))]
    pub credential: RegisterPublicKeyCredential,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct WebauthnFinishRegisterResponse {}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct WebauthnStartAuthRequest {
    pub username: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct WebauthnStartAuthResponse {
    /// Session ID.
    pub session_id: String,
    /// [`RequestChallengeResponse`] serialized as JSON string.
    #[cfg_attr(feature = "ts", ts(type = "object"))]
    pub challenge: RequestChallengeResponse,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct WebauthnFinishAuthRequest {
    /// Session ID received from [`WebauthnStartAuthResponse`].
    pub session_id: String,
    /// [`PublicKeyCredential`]
    #[cfg_attr(feature = "ts", ts(type = "object"))]
    pub credential: PublicKeyCredential,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct WalletGetInfoRequest {}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct WalletGetInfoResponse {
    pub version: String,
    pub network: String,
    pub network_byte: u8,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct TransferNftRequest {
    pub resource_address: ResourceAddress,
    pub nfts: Vec<NonFungibleId>,
    #[serde(deserialize_with = "string_or_struct")]
    pub fee_payer_account: ComponentAddressOrName,
    #[serde(deserialize_with = "string_or_struct")]
    pub source_account: ComponentAddressOrName,
    pub target_account_address: OotleAddress,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub max_fee: u64,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct TransferNftResponse {
    pub transaction_id: TransactionId,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub fee: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub fee_refunded: u64,
    pub result: FinalizeResult,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AccountsCreateStealthTransferStatementRequest {
    pub requests: Vec<TransferStatementRequest>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct TransferStatementRequest {
    pub sender_account: ComponentAddressOrName,
    pub resource_address: ResourceAddress,
    pub input_selection: InputSelection,
    pub outputs: Vec<TransferOutput>,
}

impl TransferStatementRequest {
    pub fn total_output_amount(&self) -> Amount {
        self.outputs
            .iter()
            .map(|o| Amount::from(o.blinded_amount) + o.revealed_amount)
            .sum()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub enum InputSelection {
    FromBucket { revealed_amount: Amount },
    Selection(UtxoInputSelection),
}

impl InputSelection {
    pub fn as_selection(&self) -> Option<UtxoInputSelection> {
        match self {
            InputSelection::FromBucket { .. } => None,
            InputSelection::Selection(s) => Some(*s),
        }
    }

    pub fn as_from_bucket(&self) -> Option<Amount> {
        match self {
            InputSelection::FromBucket { revealed_amount } => Some(*revealed_amount),
            InputSelection::Selection(_) => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AccountsCreateStealthTransferStatementResponse {
    pub statements: Vec<StealthTransferStatement>,
    pub lock_id: WalletLockId,
    pub signing_keys: Vec<KeyId>,
    pub utxo_signers: Vec<StealthUtxoSpendKeyId>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct StealthTransferRequest {
    pub owner_account: ComponentAddressOrName,
    pub fee_input_selection: UtxoInputSelection,
    pub input_selection: UtxoInputSelection,
    pub resource_address: ResourceAddress,
    #[serde(default, skip_serializing_if = "BadgeUsage::is_none")]
    pub badge_usage: BadgeUsage,
    pub transfers: Vec<StealthTransfer>,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub max_fee: u64,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct StealthTransfer {
    pub destination_address: OotleAddress,
    pub blinded_output_amount: u64,
    pub revealed_output_amount: Amount,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_memo: Option<Memo>,
    pub pay_to: PayTo,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct StealthTransferResponse {
    pub transaction_id: TransactionId,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AccountsAssociateStealthResourceRequest {
    pub account: ComponentAddressOrName,
    pub resource_address: ResourceAddress,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct AccountsAssociateStealthResourceResponse {}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct StealthUtxosListRequest {
    pub resource_address: ResourceAddress,
    pub account_address: Option<ComponentAddress>,
    pub filter_by_status: Option<OutputStatus>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct StealthUtxosListResponse {
    pub utxos: Vec<UtxoInfo>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct UtxoInfo {
    pub address: UtxoAddress,
    pub value: Amount,
    pub status: OutputStatus,
    pub memo: Option<Memo>,
    pub spend_condition: SpendCondition,
    pub is_burnt: bool,
    pub is_frozen: bool,
    pub is_on_chain: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct StealthUtxosDecryptValueRequest {
    pub resource_address: ResourceAddress,
    pub ids: Vec<UtxoId>,
    pub view_key_id: u64,
    pub minimum_expected_value: Option<u64>,
    pub maximum_expected_value: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct StealthUtxosDecryptValueResponse {
    pub values: HashMap<UtxoId, Option<u64>>,
}
