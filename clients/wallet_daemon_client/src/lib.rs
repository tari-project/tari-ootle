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
mod component_address;
pub use component_address::*;
pub mod error;
pub mod permissions;
pub mod serialize;
pub mod types;

use std::borrow::Borrow;

use json::Value;
use reqwest::{
    IntoUrl,
    Url,
    header::{self, AUTHORIZATION, HeaderMap},
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json as json;
use serde_json::json;
use tari_ootle_wallet_sdk::models::KeyBranch;
use types::{
    AccountsCreateFreeTestCoinsRequest,
    AccountsCreateFreeTestCoinsResponse,
    AccountsTransferRequest,
    AccountsTransferResponse,
    AuthLoginRequest,
    AuthLoginResponse,
    AuthRefreshRequest,
    CallInstructionRequest,
    ClaimBurnRequest,
    ClaimBurnResponse,
    GetNftRequest,
    GetNftResponse,
    ListNftsRequest,
    ListNftsResponse,
    MintFaucetNftRequest,
    MintFaucetNftResponse,
    ProofsCancelRequest,
    ProofsCancelResponse,
    ProofsFinalizeRequest,
    ProofsFinalizeResponse,
    ProofsGenerateRequest,
    ProofsGenerateResponse,
    TransferNftRequest,
    TransferNftResponse,
    WebRtcStartRequest,
    WebRtcStartResponse,
};

use crate::{
    error::WalletDaemonClientError,
    types::{
        AccountGetByKeyIndexRequest,
        AccountGetDefaultRequest,
        AccountGetRequest,
        AccountGetResponse,
        AccountSetDefaultRequest,
        AccountSetDefaultResponse,
        AccountsAssociateStealthResourceRequest,
        AccountsAssociateStealthResourceResponse,
        BurnProofsListRequest,
        BurnProofsListResponse,
        AccountsCreateOrGetRequest,
        AccountsCreateOrGetResponse,
        AccountsCreateRequest,
        AccountsCreateResponse,
        AccountsCreateStealthTransferStatementRequest,
        AccountsCreateStealthTransferStatementResponse,
        AccountsGetBalancesRequest,
        AccountsGetBalancesResponse,
        AccountsListRequest,
        AccountsListResponse,
        AccountsRenameRequest,
        AccountsRenameResponse,
        AuthGetMethodResponse,
        AuthListSessionsRequest,
        AuthListSessionsResponse,
        AuthRevokeTokenRequest,
        AuthRevokeTokenResponse,
        ClaimValidatorFeesRequest,
        ClaimValidatorFeesResponse,
        ConfidentialCreateOutputProofRequest,
        ConfidentialCreateOutputProofResponse,
        ConfidentialTransferRequest,
        ConfidentialTransferResponse,
        ConfidentialViewVaultBalanceRequest,
        ConfidentialViewVaultBalanceResponse,
        EncodedJwtString,
        GetValidatorFeesRequest,
        GetValidatorFeesResponse,
        KeysCreateRequest,
        KeysCreateResponse,
        KeysListRequest,
        KeysListResponse,
        KeysSetActiveRequest,
        KeysSetActiveResponse,
        PublishTemplateRequest,
        PublishTemplateResponse,
        SettingsGetResponse,
        SettingsSetRequest,
        SettingsSetResponse,
        StealthTransferRequest,
        StealthTransferResponse,
        StealthUtxosDecryptValueRequest,
        StealthUtxosDecryptValueResponse,
        StealthUtxosListRequest,
        StealthUtxosListResponse,
        SubstatesGetRequest,
        SubstatesGetResponse,
        SubstatesListRequest,
        SubstatesListResponse,
        TemplatesGetRequest,
        TemplatesGetResponse,
        TemplatesListAuthoredRequest,
        TemplatesListAuthoredResponse,
        TransactionGetAllRequest,
        TransactionGetAllResponse,
        TransactionGetRequest,
        TransactionGetResponse,
        TransactionGetResultRequest,
        TransactionGetResultResponse,
        TransactionSubmitDryRunRequest,
        TransactionSubmitDryRunResponse,
        TransactionSubmitManifestRequest,
        TransactionSubmitManifestResponse,
        TransactionSubmitRequest,
        TransactionSubmitResponse,
        TransactionWaitResultRequest,
        TransactionWaitResultResponse,
        WalletGetInfoRequest,
        WalletGetInfoResponse,
        WebauthnAlreadyRegisteredRequest,
        WebauthnAlreadyRegisteredResponse,
        WebauthnFinishRegisterRequest,
        WebauthnFinishRegisterResponse,
        WebauthnStartAuthRequest,
        WebauthnStartAuthResponse,
        WebauthnStartRegisterRequest,
        WebauthnStartRegisterResponse,
    },
};

#[derive(Debug, Clone)]
pub struct WalletDaemonClient {
    client: reqwest::Client,
    endpoint: Url,
    request_id: i64,
    token: Option<EncodedJwtString>,
}

impl WalletDaemonClient {
    pub fn connect<T: IntoUrl>(endpoint: T, token: Option<EncodedJwtString>) -> Result<Self, WalletDaemonClientError> {
        let client = reqwest::Client::builder()
            .default_headers({
                let mut headers = HeaderMap::with_capacity(1);
                headers.insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
                headers
            })
            // Enable cookie storage so that HttpOnly refresh token cookies set by the server
            // are automatically stored and sent back on subsequent requests (e.g. auth.refresh).
            .cookie_store(true)
            .build()?;

        Ok(Self {
            client,
            endpoint: endpoint.into_url()?,
            request_id: 0,
            token,
        })
    }

    pub fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    pub fn set_auth_token(&mut self, token: EncodedJwtString) -> &mut Self {
        self.token = Some(token);
        self
    }

    /// Returns general information about the wallet, including the wallet's public key and network.
    pub async fn get_wallet_info(&mut self) -> Result<WalletGetInfoResponse, WalletDaemonClientError> {
        self.send_request("wallet.get_info", &WalletGetInfoRequest {}).await
    }

    /// Derives the next key for the given [`KeyBranch`].
    pub async fn create_key(&mut self, branch: KeyBranch) -> Result<KeysCreateResponse, WalletDaemonClientError> {
        self.send_request("keys.create", &KeysCreateRequest {
            branch,
            specific_index: None,
        })
        .await
    }

    /// Derives a key at a specific index for the given [`KeyBranch`].
    pub async fn create_specific_key(
        &mut self,
        branch: KeyBranch,
        index: u64,
    ) -> Result<KeysCreateResponse, WalletDaemonClientError> {
        self.send_request("keys.create", &KeysCreateRequest {
            branch,
            specific_index: Some(index),
        })
        .await
    }

    /// Sets the active key index used for signing transactions.
    pub async fn set_active_key(&mut self, index: u64) -> Result<KeysSetActiveResponse, WalletDaemonClientError> {
        self.send_request("keys.set_active", &KeysSetActiveRequest { index })
            .await
    }

    /// Lists all derived keys for the given [`KeyBranch`].
    pub async fn list_keys(&mut self, branch: KeyBranch) -> Result<KeysListResponse, WalletDaemonClientError> {
        self.send_request("keys.list", &KeysListRequest { branch }).await
    }

    /// Fetches a transaction by ID.
    pub async fn get_transaction<T: Borrow<TransactionGetRequest>>(
        &mut self,
        request: T,
    ) -> Result<TransactionGetResponse, WalletDaemonClientError> {
        self.send_request("transactions.get", request.borrow()).await
    }

    /// Lists transactions with pagination.
    pub async fn get_transactions_all<T: Borrow<TransactionGetAllRequest>>(
        &mut self,
        request: T,
    ) -> Result<TransactionGetAllResponse, WalletDaemonClientError> {
        self.send_request("transactions.list", request.borrow()).await
    }

    /// Fetches the finalized result of a transaction without blocking.
    pub async fn get_transaction_result<T: Borrow<TransactionGetResultRequest>>(
        &mut self,
        request: T,
    ) -> Result<TransactionGetResultResponse, WalletDaemonClientError> {
        self.send_request("transactions.get_result", request.borrow()).await
    }

    /// Blocks until the transaction is finalized or the timeout is reached.
    pub async fn wait_transaction_result<T: Borrow<TransactionWaitResultRequest>>(
        &mut self,
        request: T,
    ) -> Result<TransactionWaitResultResponse, WalletDaemonClientError> {
        self.send_request("transactions.wait_result", request.borrow()).await
    }

    /// Submits a transaction to the network for processing.
    pub async fn submit_transaction<T: Borrow<TransactionSubmitRequest>>(
        &mut self,
        request: T,
    ) -> Result<TransactionSubmitResponse, WalletDaemonClientError> {
        self.send_request("transactions.submit", request.borrow()).await
    }

    /// Submits a transaction as a dry run without committing it to the network.
    pub async fn submit_transaction_dry_run<T: Borrow<TransactionSubmitDryRunRequest>>(
        &mut self,
        request: T,
    ) -> Result<TransactionSubmitDryRunResponse, WalletDaemonClientError> {
        self.send_request("transactions.submit_dry_run", request.borrow()).await
    }

    /// Submits a single instruction for execution as a transaction.
    pub async fn submit_instruction<T: Borrow<CallInstructionRequest>>(
        &mut self,
        request: T,
    ) -> Result<TransactionSubmitResponse, WalletDaemonClientError> {
        self.send_request("transactions.submit_instruction", request.borrow())
            .await
    }

    /// Submits a transaction manifest for execution.
    pub async fn submit_manifest<T: Borrow<TransactionSubmitManifestRequest>>(
        &mut self,
        request: T,
    ) -> Result<TransactionSubmitManifestResponse, WalletDaemonClientError> {
        self.send_request("transactions.submit_manifest", request.borrow())
            .await
    }

    /// Creates a new account with an optional name and key index.
    pub async fn create_account<T: Borrow<AccountsCreateRequest>>(
        &mut self,
        request: T,
    ) -> Result<AccountsCreateResponse, WalletDaemonClientError> {
        self.send_request("accounts.create", request.borrow()).await
    }

    /// Returns an existing account by name, or creates it if it does not exist.
    pub async fn create_or_get_account<T: Borrow<AccountsCreateOrGetRequest>>(
        &mut self,
        request: T,
    ) -> Result<AccountsCreateOrGetResponse, WalletDaemonClientError> {
        self.send_request("accounts.create_or_get", request.borrow()).await
    }

    /// Associates a stealth resource address with an account for tracking stealth outputs.
    pub async fn associate_stealth_resource<T: Borrow<AccountsAssociateStealthResourceRequest>>(
        &mut self,
        request: T,
    ) -> Result<AccountsAssociateStealthResourceResponse, WalletDaemonClientError> {
        self.send_request("accounts.associate_stealth_resource", request.borrow())
            .await
    }

    /// Returns the balances for all vaults in an account, optionally refreshing from the network.
    pub async fn get_account_balances<T: Borrow<AccountsGetBalancesRequest>>(
        &mut self,
        request: T,
    ) -> Result<AccountsGetBalancesResponse, WalletDaemonClientError> {
        self.send_request("accounts.get_balances", request.borrow()).await
    }

    /// Returns unclaimed validator fees for the given shard range.
    pub async fn get_validator_fees<T: Borrow<GetValidatorFeesRequest>>(
        &mut self,
        request: T,
    ) -> Result<GetValidatorFeesResponse, WalletDaemonClientError> {
        self.send_request("validators.get_fees", request.borrow()).await
    }

    /// Claims accumulated validator fees for the given shard range.
    pub async fn claim_validator_fees<T: Borrow<ClaimValidatorFeesRequest>>(
        &mut self,
        request: T,
    ) -> Result<ClaimValidatorFeesResponse, WalletDaemonClientError> {
        self.send_request("validators.claim_fees", request.borrow()).await
    }

    /// Lists accounts with pagination.
    pub async fn list_accounts(
        &mut self,
        offset: u64,
        limit: u64,
    ) -> Result<AccountsListResponse, WalletDaemonClientError> {
        self.send_request("accounts.list", &AccountsListRequest { offset, limit })
            .await
    }

    /// Fetches an account by name or component address.
    pub async fn accounts_get(
        &mut self,
        name_or_address: ComponentAddressOrName,
    ) -> Result<AccountGetResponse, WalletDaemonClientError> {
        self.send_request("accounts.get", &AccountGetRequest { name_or_address })
            .await
    }

    /// Fetches an account by its owner key index.
    pub async fn accounts_get_by_key_index(
        &mut self,
        key_index: u64,
    ) -> Result<AccountGetResponse, WalletDaemonClientError> {
        self.send_request("accounts.get_by_key_index", &AccountGetByKeyIndexRequest { key_index })
            .await
    }

    /// Returns the default account.
    pub async fn accounts_get_default(&mut self) -> Result<AccountGetResponse, WalletDaemonClientError> {
        self.send_request("accounts.get_default", &AccountGetDefaultRequest {})
            .await
    }

    /// Sets the default account used for operations when no account is specified.
    pub async fn accounts_set_default(
        &mut self,
        account: ComponentAddressOrName,
    ) -> Result<AccountSetDefaultResponse, WalletDaemonClientError> {
        self.send_request("accounts.set_default", &AccountSetDefaultRequest { account })
            .await
    }

    /// Renames an account.
    pub async fn accounts_rename(
        &mut self,
        account: ComponentAddressOrName,
        new_name: String,
    ) -> Result<AccountsRenameResponse, WalletDaemonClientError> {
        self.send_request("accounts.rename", &AccountsRenameRequest { account, new_name })
            .await
    }

    /// Transfers resources from one account to a destination public key.
    pub async fn accounts_transfer<T: Borrow<AccountsTransferRequest>>(
        &mut self,
        req: T,
    ) -> Result<AccountsTransferResponse, WalletDaemonClientError> {
        self.send_request("accounts.transfer", req.borrow()).await
    }

    /// Performs a confidential (shielded) transfer between accounts.
    pub async fn accounts_confidential_transfer<T: Borrow<ConfidentialTransferRequest>>(
        &mut self,
        req: T,
    ) -> Result<ConfidentialTransferResponse, WalletDaemonClientError> {
        self.send_request("accounts.confidential_transfer", req.borrow()).await
    }

    /// Performs a stealth transfer, sending resources to a one-time stealth address.
    pub async fn accounts_stealth_transfer<T: Borrow<StealthTransferRequest>>(
        &mut self,
        req: T,
    ) -> Result<StealthTransferResponse, WalletDaemonClientError> {
        self.send_request("accounts.stealth_transfer", req.borrow()).await
    }

    /// Creates a stealth transfer statement that can be shared with the recipient for claiming.
    pub async fn accounts_create_stealth_transfer_statement<
        T: Borrow<AccountsCreateStealthTransferStatementRequest>,
    >(
        &mut self,
        req: T,
    ) -> Result<AccountsCreateStealthTransferStatementResponse, WalletDaemonClientError> {
        self.send_request("accounts.create_stealth_transfer_statement", req.borrow())
            .await
    }

    /// Lists the available burn proof files from the configured burn proofs directory.
    pub async fn list_burn_proofs<T: Borrow<BurnProofsListRequest>>(
        &mut self,
        req: T,
    ) -> Result<BurnProofsListResponse, WalletDaemonClientError> {
        self.send_request("burn_proofs.list", req.borrow()).await
    }

    /// Claims a burn transaction, converting burned Minotari into Ootle funds.
    pub async fn claim_burn<T: Borrow<ClaimBurnRequest>>(
        &mut self,
        req: T,
    ) -> Result<ClaimBurnResponse, WalletDaemonClientError> {
        self.send_request("accounts.claim_burn", req.borrow()).await
    }

    /// Generates a confidential transfer proof for use in a confidential transaction.
    pub async fn confidential_create_transfer_proof<T: Borrow<ProofsGenerateRequest>>(
        &mut self,
        req: T,
    ) -> Result<ProofsGenerateResponse, WalletDaemonClientError> {
        self.send_request("confidential.create_transfer_proof", req.borrow())
            .await
    }

    /// Cancels a previously generated confidential transfer proof.
    pub async fn cancel_transfer_proof<T: Borrow<ProofsCancelRequest>>(
        &mut self,
        req: T,
    ) -> Result<ProofsCancelResponse, WalletDaemonClientError> {
        self.send_request("confidential.cancel", req.borrow()).await
    }

    /// Finalizes a confidential transfer proof, making it ready for submission.
    pub async fn finalize_transfer_proof<T: Borrow<ProofsFinalizeRequest>>(
        &mut self,
        req: T,
    ) -> Result<ProofsFinalizeResponse, WalletDaemonClientError> {
        self.send_request("confidential.finalize", req.borrow()).await
    }

    /// Creates a confidential output proof for a confidential deposit or withdrawal.
    pub async fn create_confidential_output_proof<T: Borrow<ConfidentialCreateOutputProofRequest>>(
        &mut self,
        req: T,
    ) -> Result<ConfidentialCreateOutputProofResponse, WalletDaemonClientError> {
        self.send_request("confidential.create_output_proof", req.borrow())
            .await
    }

    /// Creates free test coins for an account. Only available on test networks.
    pub async fn create_free_test_coins<T: Borrow<AccountsCreateFreeTestCoinsRequest>>(
        &mut self,
        req: T,
    ) -> Result<AccountsCreateFreeTestCoinsResponse, WalletDaemonClientError> {
        self.send_request("accounts.create_free_test_coins", req.borrow()).await
    }

    /// Mints one or more NFTs from the faucet into an account.
    pub async fn mint_faucet_nft<T: Borrow<MintFaucetNftRequest>>(
        &mut self,
        req: T,
    ) -> Result<MintFaucetNftResponse, WalletDaemonClientError> {
        self.send_request("nfts.mint_faucet_nft", req.borrow()).await
    }

    /// Fetches a specific NFT by its resource address and ID.
    pub async fn get_account_nft<T: Borrow<GetNftRequest>>(
        &mut self,
        req: T,
    ) -> Result<GetNftResponse, WalletDaemonClientError> {
        self.send_request("nfts.get", req.borrow()).await
    }

    /// Lists NFTs held by an account with pagination.
    pub async fn list_account_nfts<T: Borrow<ListNftsRequest>>(
        &mut self,
        req: T,
    ) -> Result<ListNftsResponse, WalletDaemonClientError> {
        self.send_request("nfts.list", req.borrow()).await
    }

    /// Transfers an NFT from one account to another.
    pub async fn transfer_nft<T: Borrow<TransferNftRequest>>(
        &mut self,
        req: T,
    ) -> Result<TransferNftResponse, WalletDaemonClientError> {
        self.send_request("nfts.transfer", req.borrow()).await
    }

    /// Decrypts and returns the confidential balance of a vault using the provided view key.
    /// The view key must correspond to the public view key of the Resource the vault holds.
    /// If the resource is not confidential, or has no view key configured, this will return an error.
    pub async fn view_vault_balance<T: Borrow<ConfidentialViewVaultBalanceRequest>>(
        &mut self,
        req: T,
    ) -> Result<ConfidentialViewVaultBalanceResponse, WalletDaemonClientError> {
        self.send_request("confidential.view_vault_balance", req.borrow()).await
    }

    /// Requests the current required authentication method to use when authenticating with the wallet daemon.
    pub async fn get_auth_method(&mut self) -> Result<AuthGetMethodResponse, WalletDaemonClientError> {
        self.send_request("auth.method", &()).await
    }

    /// Requests a JWT authentication token with the specified permissions.
    pub async fn auth_request<T: Borrow<AuthLoginRequest>>(
        &mut self,
        req: T,
    ) -> Result<AuthLoginResponse, WalletDaemonClientError> {
        self.send_request("auth.request", req.borrow()).await
    }

    /// Refreshes an authentication token using the refresh token cookie.
    pub async fn auth_refresh(&mut self) -> Result<AuthLoginResponse, WalletDaemonClientError> {
        self.send_request("auth.refresh", &AuthRefreshRequest {}).await
    }

    /// Revokes an active JWT token, invalidating further use.
    pub async fn auth_revoke<T: Borrow<AuthRevokeTokenRequest>>(
        &mut self,
        req: T,
    ) -> Result<AuthRevokeTokenResponse, WalletDaemonClientError> {
        self.send_request("auth.revoke", req.borrow()).await
    }

    /// Lists all active auth sessions.
    pub async fn auth_list_sessions<T: Borrow<AuthListSessionsRequest>>(
        &mut self,
        req: T,
    ) -> Result<AuthListSessionsResponse, WalletDaemonClientError> {
        self.send_request("auth.list_sessions", req.borrow()).await
    }

    /// Initiates a WebRTC signalling session with the wallet daemon.
    pub async fn webrtc_start<T: Borrow<WebRtcStartRequest>>(
        &mut self,
        req: T,
    ) -> Result<WebRtcStartResponse, WalletDaemonClientError> {
        self.send_request("webrtc.start", req.borrow()).await
    }

    /// Publishes a WASM template to the network.
    pub async fn publish_template<T: Borrow<PublishTemplateRequest>>(
        &mut self,
        request: T,
    ) -> Result<PublishTemplateResponse, WalletDaemonClientError> {
        self.send_request("transactions.publish_template", request.borrow())
            .await
    }

    /// Lists stealth UTXOs for an account, with optional resource filtering.
    pub async fn stealth_utxos_list<T: Borrow<StealthUtxosListRequest>>(
        &mut self,
        request: T,
    ) -> Result<StealthUtxosListResponse, WalletDaemonClientError> {
        self.send_request("stealth_utxos.list", request.borrow()).await
    }

    /// Decrypts the value of a stealth UTXO using the provided view key.
    /// The view key must correspond to the public view key of the Resource the UTXO represents.
    /// If the resource is not stealth, or has no view key configured, this will return an error.
    pub async fn stealth_utxos_decrypt_value<T: Borrow<StealthUtxosDecryptValueRequest>>(
        &mut self,
        request: T,
    ) -> Result<StealthUtxosDecryptValueResponse, WalletDaemonClientError> {
        self.send_request("stealth_utxos.decrypt_value", request.borrow()).await
    }

    /// Returns the wallet daemon's current settings.
    pub async fn get_settings(&mut self) -> Result<SettingsGetResponse, WalletDaemonClientError> {
        self.send_request("settings.get", &()).await
    }

    /// Updates the wallet daemon's settings.
    pub async fn set_settings<T: Borrow<SettingsSetRequest>>(
        &mut self,
        req: T,
    ) -> Result<SettingsSetResponse, WalletDaemonClientError> {
        self.send_request("settings.set", req.borrow()).await
    }

    /// Fetches a substate by its address from the network.
    pub async fn get_substate<T: Borrow<SubstatesGetRequest>>(
        &mut self,
        req: T,
    ) -> Result<SubstatesGetResponse, WalletDaemonClientError> {
        self.send_request("substates.get", req.borrow()).await
    }

    /// Lists substates owned by an account or matching a filter.
    pub async fn list_substates<T: Borrow<SubstatesListRequest>>(
        &mut self,
        req: T,
    ) -> Result<SubstatesListResponse, WalletDaemonClientError> {
        self.send_request("substates.list", req.borrow()).await
    }

    /// Fetches a template by its address, including its function definitions.
    pub async fn get_template<T: Borrow<TemplatesGetRequest>>(
        &mut self,
        req: T,
    ) -> Result<TemplatesGetResponse, WalletDaemonClientError> {
        self.send_request("templates.get", req.borrow()).await
    }

    /// Lists templates authored by the wallet's public key.
    pub async fn list_authored_templates<T: Borrow<TemplatesListAuthoredRequest>>(
        &mut self,
        req: T,
    ) -> Result<TemplatesListAuthoredResponse, WalletDaemonClientError> {
        self.send_request("templates.list_authored", req.borrow()).await
    }

    /// Checks whether a WebAuthn credential has already been registered.
    pub async fn webauthn_already_registered<T: Borrow<WebauthnAlreadyRegisteredRequest>>(
        &mut self,
        req: T,
    ) -> Result<WebauthnAlreadyRegisteredResponse, WalletDaemonClientError> {
        self.send_request("webauthn.already_registered", req.borrow()).await
    }

    /// Starts a WebAuthn credential registration flow, returning a challenge.
    pub async fn webauthn_start_registration<T: Borrow<WebauthnStartRegisterRequest>>(
        &mut self,
        req: T,
    ) -> Result<WebauthnStartRegisterResponse, WalletDaemonClientError> {
        self.send_request("webauthn.reg_start", req.borrow()).await
    }

    /// Completes a WebAuthn credential registration flow.
    pub async fn webauthn_finish_registration<T: Borrow<WebauthnFinishRegisterRequest>>(
        &mut self,
        req: T,
    ) -> Result<WebauthnFinishRegisterResponse, WalletDaemonClientError> {
        self.send_request("webauthn.reg_finish", req.borrow()).await
    }

    /// Starts a WebAuthn authentication flow, returning a challenge.
    pub async fn webauthn_start_auth<T: Borrow<WebauthnStartAuthRequest>>(
        &mut self,
        req: T,
    ) -> Result<WebauthnStartAuthResponse, WalletDaemonClientError> {
        self.send_request("webauthn.auth_start", req.borrow()).await
    }

    fn next_request_id(&mut self) -> i64 {
        self.request_id += 1;
        self.request_id
    }

    async fn jrpc_call<T: Serialize>(&mut self, method: &str, params: &T) -> Result<Value, WalletDaemonClientError> {
        let request_json = json!(
            {
                "jsonrpc": "2.0",
                "id": self.next_request_id(),
                "method": method,
                "params": params,
            }
        );
        let mut builder = self.client.post(self.endpoint.clone());
        if let Some(token) = &self.token {
            // If we don't have the token and the method is anything else than "auth.login" it will fail.
            builder = builder.header(AUTHORIZATION, format!("Bearer {}", token.as_str()));
        }
        let resp = builder.body(request_json.to_string()).send().await?;
        let val = resp.json().await?;
        jsonrpc_result(val)
    }

    async fn send_request<T: Serialize, R: DeserializeOwned>(
        &mut self,
        method: &str,
        params: &T,
    ) -> Result<R, WalletDaemonClientError> {
        let params = json::to_value(params).map_err(|e| WalletDaemonClientError::SerializeRequest {
            source: e,
            method: method.to_string(),
        })?;
        let resp = self.jrpc_call(method, &params).await?;
        match serde_json::from_value(resp) {
            Ok(r) => Ok(r),
            Err(e) => Err(WalletDaemonClientError::DeserializeResponse {
                source: e,
                method: method.to_string(),
            }),
        }
    }
}

fn jsonrpc_result(val: json::Value) -> Result<json::Value, WalletDaemonClientError> {
    if let Some(err) = val.get("error") {
        let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        let message = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown error")
            .to_string();
        return Err(WalletDaemonClientError::from_error_code(code, message));
    }

    let result = val
        .get("result")
        .ok_or_else(|| WalletDaemonClientError::InvalidResponse {
            message: "Missing result field".to_string(),
        })?;
    Ok(result.clone())
}
