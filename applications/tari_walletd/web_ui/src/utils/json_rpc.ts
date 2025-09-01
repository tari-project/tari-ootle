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
  AccountsTransferRequest,
  AccountsTransferResponse,
  AuthGetAllJwtRequest,
  AuthGetAllJwtResponse,
  AuthGetMethodResponse,
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
  RevealFundsRequest,
  RevealFundsResponse,
  SettingsGetResponse,
  SettingsSetRequest,
  SettingsSetResponse,
  StealthTransferRequest,
  StealthTransferResponse,
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
  TransactionSubmitManifestRequest,
  TransactionSubmitManifestResponse,
  TransactionSubmitRequest,
  TransactionSubmitResponse,
  TransactionWaitResultRequest,
  TransactionWaitResultResponse,
  TransferNftRequest,
  TransferNftResponse,
  WalletGetInfoResponse,
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
import { WalletDaemonClient } from "@tari-project/wallet_jrpc_client";
import useAuthStore from "@store/authStore";
import { AUTH_TOKEN_FOR_NONE_AUTH } from "@routes/Auth/Auth";

let clientInstance: WalletDaemonClient | null = null;
let pendingClientInstance: Promise<WalletDaemonClient> | null = null;
let unAuthenticatedClient: Promise<WalletDaemonClient> | null = null;
let outerAddress: URL | null = null;
const DEFAULT_WALLET_ADDRESS = new URL(
  import.meta.env.VITE_DAEMON_JRPC_ADDRESS ||
    import.meta.env.VITE_JSON_RPC_ADDRESS ||
    import.meta.env.VITE_JRPC_ADDRESS ||
    "http://localhost:9000",
);

export async function getClientAddress(): Promise<URL> {
  try {
    let resp = await fetch("/json_rpc_address");
    if (resp.status === 200) {
      const url = await resp.text();
      try {
        return new URL(url);
      } catch (e) {
        throw new Error(`Invalid URL: ${url} : {e}`);
      }
    }
  } catch (e) {
    console.warn(e);
  }

  return DEFAULT_WALLET_ADDRESS;
}

function getAuthToken() {
  const authToken = useAuthStore.getState().authToken;
  return authToken && authToken !== AUTH_TOKEN_FOR_NONE_AUTH ? authToken : null;
}

async function client() {
  const authToken = getAuthToken();

  if (pendingClientInstance) {
    return pendingClientInstance;
  }

  if (clientInstance) {
    if (authToken) {
      clientInstance.setToken(authToken);
    }
    if (!clientInstance.isAuthenticated()) {
      return await authenticateClient(clientInstance).then(() => clientInstance!);
    }
    return Promise.resolve(clientInstance);
  }

  const getAddress = !outerAddress ? getClientAddress() : Promise.resolve(DEFAULT_WALLET_ADDRESS);

  pendingClientInstance = getAddress.then(async (addr) => {
    const client = WalletDaemonClient.usingFetchTransport(addr.toString());
    if (authToken) {
      client.setToken(authToken);
    } else {
      await authenticateClient(client);
    }
    outerAddress = addr;
    clientInstance = client;
    pendingClientInstance = null;
    return client;
  });

  return pendingClientInstance;
}

export async function unauthenticated_client() {
  if (unAuthenticatedClient) {
    return unAuthenticatedClient;
  }

  const getAddress = !outerAddress ? getClientAddress() : Promise.resolve(DEFAULT_WALLET_ADDRESS);
  unAuthenticatedClient = getAddress.then(async (addr) => {
    return WalletDaemonClient.usingFetchTransport(addr.toString());
  });
  return unAuthenticatedClient;
}

async function authenticateClient(client: WalletDaemonClient, webauthnFinishAuthRequest?: WebauthnFinishAuthRequest) {
  const auth_token = await client.authRequest(["Admin"], webauthnFinishAuthRequest);
  await client.authAccept(auth_token, auth_token);
}

export const authGetMethod = (): Promise<AuthGetMethodResponse> =>
  unauthenticated_client().then((c) => c.authGetMethod());

export const webauthnAlreadyRegistered = (username: string): Promise<WebauthnAlreadyRegisteredResponse> =>
  unauthenticated_client().then((c) => c.webauthnAlreadyRegistered({ username }));

export const webauthnStartRegistration = (
  request: WebauthnStartRegisterRequest,
): Promise<WebauthnStartRegisterResponse> => unauthenticated_client().then((c) => c.webauthnStartRegistration(request));

export const webauthnFinishRegistration = (
  request: WebauthnFinishRegisterRequest,
): Promise<WebauthnFinishRegisterResponse> =>
  unauthenticated_client().then((c) => c.webauthnFinishRegistration(request));

export const webauthnStartAuth = (request: WebauthnStartAuthRequest): Promise<WebauthnStartAuthResponse> =>
  unauthenticated_client().then((c) => c.webauthnAuthStart(request));

export const authRevoke = (request: AuthRevokeTokenRequest): Promise<AuthRevokeTokenResponse> =>
  client().then((c) =>
    c.authRevoke(request).then((response) => {
      if (response) {
        useAuthStore.getState().setAuthToken("");
      }
      return response;
    }),
  );
export const authGetAllJwt = (request: AuthGetAllJwtRequest): Promise<AuthGetAllJwtResponse> =>
  client().then((c) => c.authGetAllJwt(request));

// settings
export const settingsGet = (): Promise<SettingsGetResponse> => client().then((c) => c.settingsGet());
export const settingsSet = (request: SettingsSetRequest): Promise<SettingsSetResponse> =>
  client().then((c) => c.settingsSet(request));

// webrtc
export const webrtcStart = (request: WebRtcStartRequest): Promise<WebRtcStartResponse> =>
  client().then((c) => c.webrtcStart(request));

// rpc
export const rpcDiscover = (): Promise<string> => client().then((c) => c.rpcDiscover());

// keys
export const keysCreate = (request: KeysCreateRequest): Promise<KeysCreateResponse> =>
  client().then((c) => c.createKey(request));
export const keysList = (request: KeysListRequest): Promise<KeysListResponse> =>
  client().then((c) => c.listKeys(request));
export const keysSetActive = (request: KeysSetActiveRequest): Promise<KeysSetActiveResponse> =>
  client().then((c) => c.keysSetActive(request));

export const transactionsSubmit = (request: TransactionSubmitRequest): Promise<TransactionSubmitResponse> =>
  client().then((c) => c.submitTransaction(request));
export const submitTransactionDryRun = (request: TransactionSubmitRequest): Promise<TransactionSubmitResponse> =>
  client().then((c) => c.submitTransactionDryRun(request));
export const transactionsGet = (request: TransactionGetRequest): Promise<TransactionGetResponse> =>
  client().then((c) => c.transactionsGet(request));
export const transactionsGetResult = (request: TransactionGetResultRequest): Promise<TransactionGetResultResponse> =>
  client().then((c) => c.getTransactionResult(request));
export const transactionsWaitResult = (request: TransactionWaitResultRequest): Promise<TransactionWaitResultResponse> =>
  client().then((c) => c.waitForTransactionResult(request));
export const transactionsGetAll = (request: TransactionGetAllRequest): Promise<TransactionGetAllResponse> =>
  client().then((c) => c.transactionsList(request));

export const transactionsPublishTemplate = (request: PublishTemplateRequest): Promise<PublishTemplateResponse> =>
  client().then((c) => c.publishTemplate(request));

export const transactionsSubmitManifest = (
  request: TransactionSubmitManifestRequest,
): Promise<TransactionSubmitManifestResponse> => client().then((c) => c.submitTransactionManifest(request));

// accounts
export const accountsRevealFunds = (request: RevealFundsRequest): Promise<RevealFundsResponse> =>
  client().then((c) => c.accountsRevealFunds(request));
export const accountsClaimBurn = (request: ClaimBurnRequest): Promise<ClaimBurnResponse> =>
  client().then((c) => c.accountsClaimBurn(request));
export const accountsCreate = (request: AccountsCreateRequest): Promise<AccountsCreateResponse> =>
  client().then((c) => c.accountsCreate(request));

export const accountsList = (request: AccountsListRequest): Promise<AccountsListResponse> =>
  client().then((c) => c.accountsList(request));
export const accountsGetBalances = (request: AccountsGetBalancesRequest): Promise<AccountsGetBalancesResponse> =>
  client().then((c) => c.accountsGetBalances(request));
export const accountsGet = (request: AccountGetRequest): Promise<AccountGetResponse> =>
  client().then((c) => c.accountsGet(request));

export const accountsTransfer = (request: AccountsTransferRequest): Promise<AccountsTransferResponse> =>
  client().then((c) => c.accountsTransfer(request));
export const accountsConfidentialTransfer = (
  request: ConfidentialTransferRequest,
): Promise<ConfidentialTransferResponse> => client().then((c) => c.confidentialTransfer(request));
export const accountsAssociateStealthResource = (
  request: AccountsAssociateStealthResourceRequest,
): Promise<AccountsAssociateStealthResourceResponse> =>
  client().then((c) => c.accountsAssociateStealthResource(request));
export const accountsStealthTransfer = (request: StealthTransferRequest): Promise<StealthTransferResponse> =>
  client().then((c) => c.stealthTransfer(request));
export const accountsSetDefault = (request: AccountSetDefaultRequest): Promise<AccountSetDefaultResponse> =>
  client().then((c) => c.accountsSetDefault(request));
export const accountsCreateFreeTestCoins = (
  request: AccountsCreateFreeTestCoinsRequest,
): Promise<AccountsCreateFreeTestCoinsResponse> => client().then((c) => c.createFreeTestCoins(request));
export const mintFaucetNfts = (request: MintFaucetNftRequest): Promise<MintFaucetNftResponse> =>
  client().then((c) => c.mintFaucetNfts(request));
export const accountsGetDefault = (request: AccountGetDefaultRequest): Promise<AccountGetResponse> =>
  client().then((c) => c.accountsGetDefault(request));

// confidential
export const confidentialViewVaultBalance = (
  request: ConfidentialViewVaultBalanceRequest,
): Promise<ConfidentialViewVaultBalanceResponse> => client().then((c) => c.viewVaultBalance(request));

// nfts
export const nftList = (request: ListNftsRequest): Promise<ListNftsResponse> =>
  client().then((c) => c.nftsList(request));

export const nftTransfer = (request: TransferNftRequest): Promise<TransferNftResponse> =>
  client().then((c) => c.nftTransfer(request));

// validators

export const validatorsClaimFees = (request: ClaimValidatorFeesRequest): Promise<ClaimValidatorFeesResponse> =>
  client().then((c) => c.validatorsClaimFees(request));
export const validatorsGetFees = (request: GetValidatorFeesRequest): Promise<GetValidatorFeesResponse> =>
  client().then((c) => c.validatorsGetFees(request));

// substates
export const substatesGet = (request: SubstatesGetRequest): Promise<SubstatesGetResponse> =>
  client().then((c) => c.substatesGet(request));

export const substatesList = (request: SubstatesListRequest): Promise<SubstatesListResponse> =>
  client().then((c) => c.substatesList(request));

// templates
export const templatesGet = (request: TemplatesGetRequest): Promise<TemplatesGetResponse> =>
  client().then((c) => c.templatesGet(request));

export const templatesListAuthored = (request: TemplatesListAuthoredRequest): Promise<TemplatesListAuthoredResponse> =>
  client().then((c) => c.templatesListAuthored(request));

// info
export const walletGetInfo = (): Promise<WalletGetInfoResponse> => client().then((c) => c.walletGetInfo());
