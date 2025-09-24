/*
 * //  Copyright 2024 The Tari Project
 * //  SPDX-License-Identifier: BSD-3-Clause
 */

import type {
  AccountGetDefaultRequest,
  AccountGetRequest,
  AccountGetResponse, AccountsAssociateStealthResourceRequest, AccountsAssociateStealthResourceResponse,
  AccountsCreateFreeTestCoinsRequest,
  AccountsCreateFreeTestCoinsResponse,
  AccountsCreateRequest,
  AccountsCreateResponse,
  AccountSetDefaultRequest,
  AccountSetDefaultResponse,
  AccountsGetBalancesRequest,
  AccountsGetBalancesResponse,
  AccountsListRequest,
  AccountsListResponse, AccountsRenameRequest, AccountsRenameResponse,
  AccountsTransferRequest,
  AccountsTransferResponse,
  AuthGetAllJwtRequest,
  AuthGetAllJwtResponse,
  AuthGetMethodResponse,
  AuthLoginRequest,
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
  GetValidatorFeesResponse,
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
  SettingsSetResponse, StealthTransferRequest, StealthTransferResponse, StealthUtxosListRequest,
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
  TransactionSubmitDryRunRequest,
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
} from "@tari-project/typescript-bindings";
import { FetchRpcTransport, RpcTransport } from "./transports";

export * as transports from "./transports";

export { substateIdToString, stringToSubstateId, rejectReasonToString };

export class WalletDaemonClient {
  private token: string | null;
  private transport: RpcTransport;
  private id: number;

  constructor(transport: RpcTransport) {
    this.token = null;
    this.transport = transport;
    this.id = 0;
  }

  public static new(transport: RpcTransport): WalletDaemonClient {
    return new WalletDaemonClient(transport);
  }

  public static usingFetchTransport(url: string): WalletDaemonClient {
    return WalletDaemonClient.new(FetchRpcTransport.new(url));
  }

  getTransport() {
    return this.transport;
  }

  public isAuthenticated() {
    return this.token !== null;
  }

  public setToken(token: string) {
    this.token = token;
  }

  public authGetMethod(): Promise<AuthGetMethodResponse> {
    return this.__invokeRpc("auth.method", {});
  }

  public authGetAllJwt(params: AuthGetAllJwtRequest): Promise<AuthGetAllJwtResponse> {
    return this.__invokeRpc("auth.get_all_jwt", params);
  }

  public async authRequest(
    permissions: string[],
    webauthnFinishAuthRequest?: WebauthnFinishAuthRequest,
  ): Promise<string> {
    // TODO: Exchange some secret credentials for a JWT
    let request: AuthLoginRequest = {
      permissions: permissions,
      duration: null,
      webauthn_finish_auth_request: webauthnFinishAuthRequest,
    };
    let resp = await this.__invokeRpc("auth.request", request);
    return resp.auth_token;
  }

  public async authAccept(adminToken: string, name: string): Promise<string> {
    let resp = await this.__invokeRpc("auth.accept", { auth_token: adminToken, name });
    this.token = resp.permissions_token;
    return this.token;
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

  public submitTransactionDryRun(params: TransactionSubmitDryRunRequest): Promise<TransactionSubmitDryRunResponse> {
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

  public rpcDiscover(): Promise<string> {
    return this.__invokeRpc("rpc.discover", {});
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


  async __invokeRpc(method: string, params: object = null) {
    const id = this.id++;
    const response = await this.transport.sendRequest<any>(
      {
        method,
        jsonrpc: "2.0",
        id: id,
        params: params || {},
      },
      { token: this.token, timeout_millis: null },
    );

    // TODO: Handle errors by throwing a custom error type

    return response;
  }
}
