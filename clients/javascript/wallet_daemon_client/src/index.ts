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
  AccountsTransferResponse,
  AuthCreateApiKeyRequest,
  AuthCreateApiKeyResponse,
  AuthCredentials,
  AuthListApiKeysRequest,
  AuthListApiKeysResponse,
  AuthListSessionsRequest,
  AuthListSessionsResponse,
  AuthGetMethodResponse,
  AuthLoginRequest,
  AuthLoginResponse,
  AuthRevokeApiKeyRequest,
  AuthRevokeApiKeyResponse,
  AuthRevokeTokenRequest,
  AuthRevokeTokenResponse,
  BurnProofsGetRequest,
  BurnProofsGetResponse,
  BurnProofsListRequest,
  BurnProofsListResponse,
  ClaimBurnRequest,
  ClaimBurnResponse,
  ClaimValidatorFeesRequest,
  ClaimValidatorFeesResponse,
  ConfidentialTransferRequest,
  ConfidentialTransferResponse,
  ConfidentialViewVaultBalanceRequest,
  ConfidentialViewVaultBalanceResponse,
  GetValidatorFeesRequest,
  GetValidatorFeesResponse,
  JrpcPermission,
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
  StealthUtxosDecryptValueRequest,
  StealthUtxosDecryptValueResponse,
  StealthUtxosListRequest,
  StealthUtxosListResponse,
  stringToSubstateId,
  substateIdToString,
  SubstatesGetRequest,
  SubstatesGetResponse,
  SubstatesListRequest,
  SubstatesListResponse,
  SwapPoolGetExchangeRateRequest,
  SwapPoolGetExchangeRateResponse,
  SwapPoolsListRequest,
  SwapPoolsListResponse,
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
  WebauthnFinishRegisterRequest,
  WebauthnFinishRegisterResponse,
  WebauthnStartAuthRequest,
  WebauthnStartAuthResponse,
  WebauthnStartRegisterRequest,
  WebauthnStartRegisterResponse,
  WebRtcStartRequest,
  WebRtcStartResponse,
  AuthRefreshResponse,
  AddressBookAddRequest,
  AddressBookAddResponse,
  AddressBookListResponse,
  AddressBookGetRequest,
  AddressBookGetResponse,
  AddressBookUpdateRequest,
  AddressBookUpdateResponse,
  AddressBookDeleteRequest,
  AddressBookDeleteResponse,
} from "@tari-project/ootle-ts-bindings";
import { FetchRpcTransport, RpcErrorResponse, RpcResponse, RpcTransport } from "./transports";

export * as transports from "./transports";

export { substateIdToString, stringToSubstateId, rejectReasonToString };

export class WalletDaemonClient<T extends RpcTransport = FetchRpcTransport> {
  private token: string | null;
  private transport: T;
  private id: number;
  private reauthEnabled: boolean = true;

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

  public setReauthenticationEnabled(enabled: boolean) {
    this.reauthEnabled = enabled;
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
    return this.sendRequest("auth.method", {});
  }

  public authListSessions(params: AuthListSessionsRequest): Promise<AuthListSessionsResponse> {
    return this.sendRequest("auth.list_sessions", params);
  }

  public async authRequest(permissions: JrpcPermission[], credentials: AuthCredentials): Promise<string> {
    let request: AuthLoginRequest = {
      permissions: permissions,
      credentials,
    };
    let resp = await this.sendRequest<AuthLoginResponse>("auth.request", request);
    return resp.token;
  }

  public authRevoke(params: AuthRevokeTokenRequest): Promise<AuthRevokeTokenResponse> {
    return this.sendRequest("auth.revoke", params);
  }

  public authRefresh(): Promise<AuthRefreshResponse> {
    return this.sendRequest("auth.refresh");
  }

  // ---------------------------------------------------------------------------
  // API key management (issue #1957). All three methods require the active
  // session to hold the `Admin` permission; the daemon rejects non-admin calls
  // at the JSON-RPC layer.
  // ---------------------------------------------------------------------------

  /**
   * Mint a new long-lived API key with the supplied permission scopes.
   *
   * The response contains the raw key material exactly once. The caller MUST
   * persist it immediately — the daemon only stores a SHA-256 hash and the
   * key cannot be retrieved later.
   *
   * If the granted permissions include `Admin`, the request must set
   * `confirm_admin: true`.
   */
  public authCreateApiKey(params: AuthCreateApiKeyRequest): Promise<AuthCreateApiKeyResponse> {
    return this.sendRequest("auth.create_api_key", params);
  }

  /**
   * List API keys. By default returns only active (non-revoked) keys; pass
   * `{ include_revoked: true }` to retrieve the full audit history.
   * Expired keys are always included. Never returns raw key material.
   */
  public authListApiKeys(
    params: AuthListApiKeysRequest = { include_revoked: false },
  ): Promise<AuthListApiKeysResponse> {
    return this.sendRequest("auth.list_api_keys", params);
  }

  /**
   * Revoke an API key by id. Revocation is immediate — the daemon's auth
   * lookup filter excludes revoked rows so any in-flight request using the
   * revoked key will fail.
   */
  public authRevokeApiKey(params: AuthRevokeApiKeyRequest): Promise<AuthRevokeApiKeyResponse> {
    return this.sendRequest("auth.revoke_api_key", params);
  }

  /**
   * Authenticate this client using an API key string. Agents submit the raw
   * key as the `Authorization: Bearer …` header on every JSON-RPC call —
   * the daemon resolves the `tw_`-prefixed bearer against the api_keys
   * table on each call, so there is no `auth.request` round-trip and no
   * JWT exchange. Equivalent to `setToken(apiKey)` but reads better at the
   * call site.
   */
  public authenticateWithApiKey(apiKey: string): void {
    this.setToken(apiKey);
  }

  public walletGetInfo(): Promise<WalletGetInfoResponse> {
    return this.sendRequest("wallet.get_info", {} as WalletGetInfoRequest);
  }

  public accountsCreate(params: AccountsCreateRequest): Promise<AccountsCreateResponse> {
    return this.sendRequest("accounts.create", params);
  }

  public accountsRename(params: AccountsRenameRequest): Promise<AccountsRenameResponse> {
    return this.sendRequest("accounts.rename", params);
  }

  public burnProofsList(params: BurnProofsListRequest): Promise<BurnProofsListResponse> {
    return this.sendRequest("burn_proofs.list", params);
  }

  public burnProofsGet(params: BurnProofsGetRequest): Promise<BurnProofsGetResponse> {
    return this.sendRequest("burn_proofs.get", params);
  }

  public accountsClaimBurn(params: ClaimBurnRequest): Promise<ClaimBurnResponse> {
    return this.sendRequest("accounts.claim_burn", params);
  }

  public accountsAssociateStealthResource(
    params: AccountsAssociateStealthResourceRequest,
  ): Promise<AccountsAssociateStealthResourceResponse> {
    return this.sendRequest("accounts.associate_stealth_resource", params);
  }

  public accountsGetBalances(params: AccountsGetBalancesRequest): Promise<AccountsGetBalancesResponse> {
    return this.sendRequest("accounts.get_balances", params);
  }

  public accountsList(params: AccountsListRequest): Promise<AccountsListResponse> {
    return this.sendRequest("accounts.list", params);
  }

  public accountsGet(params: AccountGetRequest): Promise<AccountGetResponse> {
    return this.sendRequest("accounts.get", params);
  }

  public accountsTransfer(params: AccountsTransferRequest): Promise<AccountsTransferResponse> {
    return this.sendRequest("accounts.transfer", params);
  }

  public confidentialTransfer(params: ConfidentialTransferRequest): Promise<ConfidentialTransferResponse> {
    return this.sendRequest("accounts.confidential_transfer", params);
  }

  public stealthTransfer(params: StealthTransferRequest): Promise<StealthTransferResponse> {
    return this.sendRequest("accounts.stealth_transfer", params);
  }

  public accountsGetDefault(params: AccountGetDefaultRequest): Promise<AccountGetResponse> {
    return this.sendRequest("accounts.get_default", params);
  }

  public accountsSetDefault(params: AccountSetDefaultRequest): Promise<AccountSetDefaultResponse> {
    return this.sendRequest("accounts.set_default", params);
  }

  public submitTransaction(params: TransactionSubmitRequest): Promise<TransactionSubmitResponse> {
    return this.sendRequest("transactions.submit", params);
  }

  public submitTransactionDryRun(params: TransactionSubmitRequest): Promise<TransactionSubmitDryRunResponse> {
    return this.sendRequest("transactions.submit_dry_run", params);
  }

  public submitTransactionManifest(
    params: TransactionSubmitManifestRequest,
  ): Promise<TransactionSubmitManifestResponse> {
    return this.sendRequest("transactions.submit_manifest", params);
  }

  public publishTemplate(params: PublishTemplateRequest): Promise<PublishTemplateResponse> {
    return this.sendRequest("transactions.publish_template", params);
  }

  public substatesGet(params: SubstatesGetRequest): Promise<SubstatesGetResponse> {
    return this.sendRequest("substates.get", params);
  }

  public substatesList(params: SubstatesListRequest): Promise<SubstatesListResponse> {
    return this.sendRequest("substates.list", params);
  }

  public swapPoolGetExchangeRate(
    params: SwapPoolGetExchangeRateRequest,
  ): Promise<SwapPoolGetExchangeRateResponse> {
    return this.sendRequest("swap_pools.get_exchange_rate", params);
  }

  public swapPoolsList(params: SwapPoolsListRequest): Promise<SwapPoolsListResponse> {
    return this.sendRequest("swap_pools.list", params);
  }

  public transactionsList(params: TransactionGetAllRequest): Promise<TransactionGetAllResponse> {
    return this.sendRequest("transactions.list", params);
  }

  public transactionsGet(params: TransactionGetRequest): Promise<TransactionGetResponse> {
    return this.sendRequest("transactions.get", params);
  }

  public getTransactionResult(params: TransactionGetResultRequest): Promise<TransactionGetResultResponse> {
    return this.sendRequest("transactions.get_result", params);
  }

  public waitForTransactionResult(params: TransactionWaitResultRequest): Promise<TransactionWaitResultResponse> {
    return this.sendRequest("transactions.wait_result", params);
  }

  public templatesGet(params: TemplatesGetRequest): Promise<TemplatesGetResponse> {
    return this.sendRequest("templates.get", params);
  }

  public templatesListAuthored(params: TemplatesListAuthoredRequest): Promise<TemplatesListAuthoredResponse> {
    return this.sendRequest("templates.list_authored", params);
  }

  public createFreeTestCoins(params: AccountsCreateFreeTestCoinsRequest): Promise<AccountsCreateFreeTestCoinsResponse> {
    return this.sendRequest("accounts.create_free_test_coins", params);
  }

  public createKey(params: KeysCreateRequest): Promise<KeysCreateResponse> {
    return this.sendRequest("keys.create", params);
  }

  public keysSetActive(params: KeysSetActiveRequest): Promise<KeysSetActiveResponse> {
    return this.sendRequest("keys.set_active", params);
  }

  public listKeys(params: KeysListRequest): Promise<KeysListResponse> {
    return this.sendRequest("keys.list", params);
  }

  public viewVaultBalance(params: ConfidentialViewVaultBalanceRequest): Promise<ConfidentialViewVaultBalanceResponse> {
    return this.sendRequest("confidential.view_vault_balance", params);
  }

  public nftsList(params: ListNftsRequest): Promise<ListNftsResponse> {
    return this.sendRequest("nfts.list", params);
  }

  public nftTransfer(params: TransferNftRequest): Promise<TransferNftResponse> {
    return this.sendRequest("nfts.transfer", params);
  }

  public mintFaucetNfts(params: MintFaucetNftRequest): Promise<MintFaucetNftResponse> {
    return this.sendRequest("nfts.mint_faucet_nft", params);
  }

  public validatorsClaimFees(params: ClaimValidatorFeesRequest): Promise<ClaimValidatorFeesResponse> {
    return this.sendRequest("validators.claim_fees", params);
  }

  public validatorsGetFees(params: GetValidatorFeesRequest): Promise<GetValidatorFeesResponse> {
    return this.sendRequest("validators.get_fees", params);
  }

  public webrtcStart(params: WebRtcStartRequest): Promise<WebRtcStartResponse> {
    return this.sendRequest("webrtc.start", params);
  }

  public settingsGet(): Promise<SettingsGetResponse> {
    return this.sendRequest("settings.get");
  }

  public settingsSet(params: SettingsSetRequest): Promise<SettingsSetResponse> {
    return this.sendRequest("settings.set", params);
  }

  public webauthnAlreadyRegistered(
    params: WebauthnAlreadyRegisteredRequest,
  ): Promise<WebauthnAlreadyRegisteredResponse> {
    return this.sendRequest("webauthn.already_registered", params);
  }

  public webauthnStartRegistration(params: WebauthnStartRegisterRequest): Promise<WebauthnStartRegisterResponse> {
    return this.sendRequest("webauthn.reg_start", params);
  }

  public webauthnFinishRegistration(params: WebauthnFinishRegisterRequest): Promise<WebauthnFinishRegisterResponse> {
    return this.sendRequest("webauthn.reg_finish", params);
  }

  public webauthnAuthStart(params: WebauthnStartAuthRequest): Promise<WebauthnStartAuthResponse> {
    return this.sendRequest("webauthn.auth_start", params);
  }

  public stealthUtxosList(params: StealthUtxosListRequest): Promise<StealthUtxosListResponse> {
    return this.sendRequest("stealth_utxos.list", params);
  }

  public stealthUtxosDecryptValue(params: StealthUtxosDecryptValueRequest): Promise<StealthUtxosDecryptValueResponse> {
    return this.sendRequest("stealth_utxos.decrypt_value", params);
  }

  // Address book

  public addressBookAdd(params: AddressBookAddRequest): Promise<AddressBookAddResponse> {
    return this.sendRequest("address_book.add", params);
  }

  public addressBookList(): Promise<AddressBookListResponse> {
    return this.sendRequest("address_book.list", {});
  }

  public addressBookGet(params: AddressBookGetRequest): Promise<AddressBookGetResponse> {
    return this.sendRequest("address_book.get", params);
  }

  public addressBookUpdate(params: AddressBookUpdateRequest): Promise<AddressBookUpdateResponse> {
    return this.sendRequest("address_book.update", params);
  }

  public addressBookDelete(params: AddressBookDeleteRequest): Promise<AddressBookDeleteResponse> {
    return this.sendRequest("address_book.delete", params);
  }

  async sendRequest<R>(method: string, params: object = null): Promise<R> {
    const id = this.id++;
    let response = (await this.transport.sendRequest<any>(
      {
        method,
        jsonrpc: "2.0",
        id,
        params: params || {},
      },
      { token: this.token, timeout_millis: null },
    )) as RpcResponse<R>;

    if (!this.reauthEnabled) {
      if (response.error) {
        throw new Error(`RPC Error ${response.error.code}: ${response.error.message}`, {
          cause: {
            method,
            ...response.error,
          } as RpcError,
        });
      }
      return response.result;
    }

    // If we get an unauthorized error, try refreshing the token and retrying the request once
    if (response?.error && response.error.code === 401) {
      const origError = new Error(`RPC Error ${response.error.code}: ${response.error.message}`, {
        cause: {
          method,
          ...response.error,
        } as RpcError,
      });
      // Refresh failed with 401. No point in trying it again
      if (method.startsWith("auth.")) {
        console.warn("Token refresh failed");
        this.token = null;
        throw origError;
      }
      const id = this.id++;
      const refreshResp = (await this.transport.sendRequest<any>({
        method: "auth.refresh",
        jsonrpc: "2.0",
        id,
        params: {},
      })) as RpcResponse<AuthLoginResponse>;

      if (refreshResp.error) {
        console.debug("Refresh resp", refreshResp);
        this.token = null;
        // Throw the original 401 error instead of the refresh error
        throw origError;
      }

      console.debug("Token refreshed successfully. Retrying original request.");
      this.token = refreshResp.result.token;

      // Retry the original request with the new token
      response = (await this.transport.sendRequest<any>(
        {
          method,
          jsonrpc: "2.0",
          id,
          params: params || {},
        },
        { token: this.token, timeout_millis: null },
      )) as RpcResponse<R>;
    }

    if (response.error) {
      throw new Error(`RPC Error ${response.error.code}: ${response.error.message}`, {
        cause: {
          method,
          ...response.error,
        } as RpcError,
      });
    }

    return response.result;
  }
}

export type RpcError = RpcErrorResponse & {
  method: string;
};

export function isAuthError(error: any): error is RpcError {
  return error instanceof Error && (error as any).cause?.code === 401;
}
