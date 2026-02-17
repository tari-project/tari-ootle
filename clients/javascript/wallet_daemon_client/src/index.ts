/*
 * //  Copyright 2024 The Tari Project
 * //  SPDX-License-Identifier: BSD-3-Clause
 */

import "./serialize";
import type {
  AccountGetDefaultRequest,
  AccountGetRequest,
  AccountGetResponse,
  AccountsAssociateStealthResourceRequest,
  AccountsAssociateStealthResourceResponse,
  AccountsCreateFreeTestCoinsRequest,
  AccountsCreateFreeTestCoinsResponse,
  AccountsCreateRequest,
  AccountsCreateResponse,
  AccountSetDefaultRequest,
  AccountSetDefaultResponse,
  AccountsGetBalancesRequest,
  AccountsGetBalancesResponse,
  AccountsListRequest,
  AccountsListResponse,
  AccountsRenameRequest,
  AccountsRenameResponse,
  AccountsTransferRequest,
  AccountsTransferResponse, AuthCredentials,
  AuthListSessionsRequest,
  AuthListSessionsResponse,
  AuthGetMethodResponse,
  AuthLoginRequest,
  AuthLoginResponse,
  AuthRevokeTokenRequest,
  AuthRevokeTokenResponse,
  ClaimBurnRequest,
  ClaimBurnResponse,
  ClaimValidatorFeesRequest,
  ClaimValidatorFeesResponse,
  ConfidentialTransferRequest,
  ConfidentialTransferResponse,
  ConfidentialViewVaultBalanceRequest,
  ConfidentialViewVaultBalanceResponse,
  GetValidatorFeesRequest,
  GetValidatorFeesResponse, JrpcPermission,
  KeysCreateRequest,
  KeysCreateResponse,
  KeysListRequest,
  KeysListResponse,
  KeysSetActiveRequest,
  KeysSetActiveResponse,
  ListNftsRequest,
  ListNftsResponse,
  MintFaucetNftRequest,
  MintFaucetNftResponse,
  PublishTemplateRequest,
  PublishTemplateResponse,
  rejectReasonToString,
  SettingsGetResponse,
  SettingsSetRequest,
  SettingsSetResponse,
  StealthTransferRequest,
  StealthTransferResponse,
  StealthUtxosDecryptValueRequest, StealthUtxosDecryptValueResponse,
  StealthUtxosListRequest,
  StealthUtxosListResponse,
  stringToSubstateId,
  substateIdToString,
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
  TransactionSubmitDryRunResponse,
  TransactionSubmitManifestRequest,
  TransactionSubmitManifestResponse,
  TransactionSubmitRequest,
  TransactionSubmitResponse,
  TransactionWaitResultRequest,
  TransactionWaitResultResponse,
  TransferNftRequest,
  TransferNftResponse,
  WalletGetInfoRequest,
  WalletGetInfoResponse,
  WebauthnAlreadyRegisteredRequest,
  WebauthnAlreadyRegisteredResponse,
  WebauthnFinishAuthRequest,
  WebauthnFinishRegisterRequest,
  WebauthnFinishRegisterResponse,
  WebauthnStartAuthRequest,
  WebauthnStartAuthResponse,
  WebauthnStartRegisterRequest,
  WebauthnStartRegisterResponse,
  WebRtcStartRequest,
  WebRtcStartResponse,
} from "@tari-project/ootle-ts-bindings";
import { FetchRpcTransport, RpcResponse, RpcTransport } from "./transports";

export * as transports from "./transports";

export { substateIdToString, stringToSubstateId, rejectReasonToString };

export class WalletDaemonClient<T extends RpcTransport = FetchRpcTransport> {
  private token: string | null;
  private transport: T;
  private id: number;

  constructor(transport: T) {
    this.token = null;
    this.transport = transport;
    this.id = 0;
  }

  public static new<T extends RpcTransport>(transport: T): WalletDaemonClient<T> {
    return new WalletDaemonClient(transport);
  }

  public static usingFetchTransport(url: string): WalletDaemonClient {
    return WalletDaemonClient.new(FetchRpcTransport.new(url));
  }


  public isAuthenticated() {
    return Boolean(this.token);
  }

  public setToken(token: string) {
    this.token = token;
  }

  public getToken(): string | null {
    return this.token;
  }

  public getTransport(): T {
    return this.transport;
  }

  public authGetMethod(): Promise<AuthGetMethodResponse> {
    return this.__invokeRpc("auth.method", {});
  }

  public authListSessions(params: AuthListSessionsRequest): Promise<AuthListSessionsResponse> {
    return this.__invokeRpc("auth.list_sessions", params);
  }

  public async authRequest(
    permissions: JrpcPermission[],
    credentials: AuthCredentials
  ): Promise<string> {
    let request: AuthLoginRequest = {
      permissions: permissions,
      credentials,
    };
    let resp = await this.__invokeRpc<AuthLoginResponse>("auth.request", request);
    return resp.token;
  }

  public authRevoke(params: AuthRevokeTokenRequest): Promise<AuthRevokeTokenResponse> {
    return this.__invokeRpc("auth.revoke", params);
  }

  public walletGetInfo(): Promise<WalletGetInfoResponse> {
    return this.__invokeRpc("wallet.get_info", {} as WalletGetInfoRequest);
  }

  public accountsCreate(params: AccountsCreateRequest): Promise<AccountsCreateResponse> {
    return this.__invokeRpc("accounts.create", params);
  }

  public accountsRename(params: AccountsRenameRequest): Promise<AccountsRenameResponse> {
    return this.__invokeRpc("accounts.rename", params);
  }

  public accountsClaimBurn(params: ClaimBurnRequest): Promise<ClaimBurnResponse> {
    return this.__invokeRpc("accounts.claim_burn", params);
  }

  public accountsAssociateStealthResource(params: AccountsAssociateStealthResourceRequest): Promise<AccountsAssociateStealthResourceResponse> {
    return this.__invokeRpc("accounts.associate_stealth_resource", params);
  }

  public accountsGetBalances(params: AccountsGetBalancesRequest): Promise<AccountsGetBalancesResponse> {
    return this.__invokeRpc("accounts.get_balances", params);
  }

  public accountsList(params: AccountsListRequest): Promise<AccountsListResponse> {
    return this.__invokeRpc("accounts.list", params);
  }

  public accountsGet(params: AccountGetRequest): Promise<AccountGetResponse> {
    return this.__invokeRpc("accounts.get", params);
  }

  public accountsTransfer(params: AccountsTransferRequest): Promise<AccountsTransferResponse> {
    return this.__invokeRpc("accounts.transfer", params);
  }

  public confidentialTransfer(params: ConfidentialTransferRequest): Promise<ConfidentialTransferResponse> {
    return this.__invokeRpc("accounts.confidential_transfer", params);
  }

  public stealthTransfer(params: StealthTransferRequest): Promise<StealthTransferResponse> {
    return this.__invokeRpc("accounts.stealth_transfer", params);
  }

  public accountsGetDefault(params: AccountGetDefaultRequest): Promise<AccountGetResponse> {
    return this.__invokeRpc("accounts.get_default", params);
  }

  public accountsSetDefault(params: AccountSetDefaultRequest): Promise<AccountSetDefaultResponse> {
    return this.__invokeRpc("accounts.set_default", params);
  }

  public submitTransaction(params: TransactionSubmitRequest): Promise<TransactionSubmitResponse> {
    return this.__invokeRpc("transactions.submit", params);
  }

  public submitTransactionDryRun(params: TransactionSubmitRequest): Promise<TransactionSubmitDryRunResponse> {
    return this.__invokeRpc("transactions.submit_dry_run", params);
  }

  public submitTransactionManifest(
    params: TransactionSubmitManifestRequest,
  ): Promise<TransactionSubmitManifestResponse> {
    return this.__invokeRpc("transactions.submit_manifest", params);
  }

  public publishTemplate(params: PublishTemplateRequest): Promise<PublishTemplateResponse> {
    return this.__invokeRpc("transactions.publish_template", params);
  }

  public substatesGet(params: SubstatesGetRequest): Promise<SubstatesGetResponse> {
    return this.__invokeRpc("substates.get", params);
  }

  public substatesList(params: SubstatesListRequest): Promise<SubstatesListResponse> {
    return this.__invokeRpc("substates.list", params);
  }

  public transactionsList(params: TransactionGetAllRequest): Promise<TransactionGetAllResponse> {
    return this.__invokeRpc("transactions.list", params);
  }

  public transactionsGet(params: TransactionGetRequest): Promise<TransactionGetResponse> {
    return this.__invokeRpc("transactions.get", params);
  }

  public getTransactionResult(params: TransactionGetResultRequest): Promise<TransactionGetResultResponse> {
    return this.__invokeRpc("transactions.get_result", params);
  }

  public waitForTransactionResult(params: TransactionWaitResultRequest): Promise<TransactionWaitResultResponse> {
    return this.__invokeRpc("transactions.wait_result", params);
  }

  public templatesGet(params: TemplatesGetRequest): Promise<TemplatesGetResponse> {
    return this.__invokeRpc("templates.get", params);
  }

  public templatesListAuthored(params: TemplatesListAuthoredRequest): Promise<TemplatesListAuthoredResponse> {
    return this.__invokeRpc("templates.list_authored", params);
  }

  public createFreeTestCoins(params: AccountsCreateFreeTestCoinsRequest): Promise<AccountsCreateFreeTestCoinsResponse> {
    return this.__invokeRpc("accounts.create_free_test_coins", params);
  }

  public createKey(params: KeysCreateRequest): Promise<KeysCreateResponse> {
    return this.__invokeRpc("keys.create", params);
  }

  public keysSetActive(params: KeysSetActiveRequest): Promise<KeysSetActiveResponse> {
    return this.__invokeRpc("keys.set_active", params);
  }

  public listKeys(params: KeysListRequest): Promise<KeysListResponse> {
    return this.__invokeRpc("keys.list", params);
  }

  public viewVaultBalance(params: ConfidentialViewVaultBalanceRequest): Promise<ConfidentialViewVaultBalanceResponse> {
    return this.__invokeRpc("confidential.view_vault_balance", params);
  }

  public nftsList(params: ListNftsRequest): Promise<ListNftsResponse> {
    return this.__invokeRpc("nfts.list", params);
  }

  public nftTransfer(params: TransferNftRequest): Promise<TransferNftResponse> {
    return this.__invokeRpc("nfts.transfer", params);
  }

  public mintFaucetNfts(params: MintFaucetNftRequest): Promise<MintFaucetNftResponse> {
    return this.__invokeRpc("nfts.mint_faucet_nft", params);
  }

  public validatorsClaimFees(params: ClaimValidatorFeesRequest): Promise<ClaimValidatorFeesResponse> {
    return this.__invokeRpc("validators.claim_fees", params);
  }

  public validatorsGetFees(params: GetValidatorFeesRequest): Promise<GetValidatorFeesResponse> {
    return this.__invokeRpc("validators.get_fees", params);
  }


  public webrtcStart(params: WebRtcStartRequest): Promise<WebRtcStartResponse> {
    return this.__invokeRpc("webrtc.start", params);
  }

  public settingsGet(): Promise<SettingsGetResponse> {
    return this.__invokeRpc("settings.get");
  }

  public settingsSet(params: SettingsSetRequest): Promise<SettingsSetResponse> {
    return this.__invokeRpc("settings.set", params);
  }

  public webauthnAlreadyRegistered(
    params: WebauthnAlreadyRegisteredRequest,
  ): Promise<WebauthnAlreadyRegisteredResponse> {
    return this.__invokeRpc("webauthn.already_registered", params);
  }

  public webauthnStartRegistration(params: WebauthnStartRegisterRequest): Promise<WebauthnStartRegisterResponse> {
    return this.__invokeRpc("webauthn.reg_start", params);
  }

  public webauthnFinishRegistration(params: WebauthnFinishRegisterRequest): Promise<WebauthnFinishRegisterResponse> {
    return this.__invokeRpc("webauthn.reg_finish", params);
  }

  public webauthnAuthStart(params: WebauthnStartAuthRequest): Promise<WebauthnStartAuthResponse> {
    return this.__invokeRpc("webauthn.auth_start", params);
  }

  public stealthUtxosList(params: StealthUtxosListRequest): Promise<StealthUtxosListResponse> {
    return this.__invokeRpc("stealth_utxos.list", params);
  }


  public stealthUtxosDecryptValue(params: StealthUtxosDecryptValueRequest): Promise<StealthUtxosDecryptValueResponse> {
    return this.__invokeRpc("stealth_utxos.decrypt_value", params);
  }

  async __invokeRpc<R>(method: string, params: object = null) : Promise<R> {
    const AUTH_FAIL = "AUTH_FAIL";
    const id = this.id++;
    let response = await this.transport.sendRequest<any>(
      {
        method,
        jsonrpc: "2.0",
        id: id,
        params: params || {},
      },
      { token: this.token, timeout_millis: null },
    ) as RpcResponse<R>;

    // If we get an unauthorized error, try refreshing the token and retrying the request once
    if (response?.error && response.error.code === 401) {
      // Refresh failed with 401. No point in trying it again
      if (method === "auth.refresh") {
        console.warn("Token refresh failed");
        this.token = null;
        throw new Error(`RPC Error ${response.error.code}: ${response.error.message}`, {cause: AUTH_FAIL});
      }
      const id = this.id++;
      try {
        const refreshResp = await this.transport.sendRequest<any>(
          {
            method: "auth.refresh",
            jsonrpc: "2.0",
            id: id,
            params: {}
          },
        ) as RpcResponse<AuthLoginResponse>;
        if (refreshResp.error) {
          throw new Error(`Auth refresh failed: ${refreshResp.error.code} - ${refreshResp.error.message}`, {cause: AUTH_FAIL});
        }

        this.token = refreshResp.result.token;
      } catch (err) {
        console.warn("Token refresh failed, clearing token and returning original error", err);
        this.token = null;
        throw new Error(`RPC Error ${response.error.code}: ${response.error.message}`, {cause: AUTH_FAIL});
      }

      // Retry the original request with the new token
      response = await this.transport.sendRequest<any>(
        {
          method,
          jsonrpc: "2.0",
          id: id,
          params: params || {},
        },
        { token: this.token, timeout_millis: null },
      ) as RpcResponse<R>;

    }

    if (response.error) {
      throw new Error(`RPC Error ${response.error.code}: ${response.error.message}`, {cause: response.error});
    }

    return response.result;
  }
}
