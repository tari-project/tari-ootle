/*
 * //  Copyright 2024 The Tari Project
 * //  SPDX-License-Identifier: BSD-3-Clause
 */

import type {
  GetEpochManagerStatsResponse,
  GetNetworkSyncStateResponse, GetNonFungiblesRequest, GetNonFungiblesResponse,
  GetRecentTransactionsRequest,
  GetRecentTransactionsResponse,
  GetSubstatesRequest,
  GetSubstatesResponse, GetTransactionReceiptResponse,
  IndexerAddPeerRequest,
  IndexerAddPeerResponse,
  IndexerGetConnectionsResponse,
  IndexerGetIdentityResponse,
  IndexerGetSubstateRequest,
  IndexerGetSubstateResponse,
  IndexerGetTransactionResultResponse,
  IndexerReadyResponse, ListRecentTransactionsRequest, ListRecentTransactionsResponse,
  ListSubstatesRequest,
  ListSubstatesResponse, ListTemplatesResponse, ListTransactionReceiptsRequest, ListTransactionReceiptsResponse,
  rejectReasonToString,
  stringToSubstateId,
  SubstateId,
  substateIdToString,
  TemplatesGetResponse,
  TemplatesListAuthoredRequest,
  TemplatesListAuthoredResponse,
  TransactionId, TransactionReceiptAddress,
  TransactionSubmitRequest,
  TransactionSubmitResponse,
} from "@tari-project/typescript-bindings";
import { FetchTransport, HttpTransport } from "./transports";

export * as transports from "./transports";

export { substateIdToString, stringToSubstateId, rejectReasonToString };

export class IndexerClient {
  private transport: HttpTransport;

  constructor(transport: HttpTransport) {
    this.transport = transport;
  }

  public static new(transport: HttpTransport): IndexerClient {
    return new IndexerClient(transport);
  }

  public static usingFetchTransport(url: string): IndexerClient {
    return IndexerClient.new(FetchTransport.new(url));
  }

  getTransport() {
    return this.transport;
  }

  public identityGet(): Promise<IndexerGetIdentityResponse> {
    return this.transport.sendGet(`identity`, {});
  }

  public waitUntilReady(): Promise<IndexerReadyResponse> {
    return this.transport.sendGet(`wait-until-ready`, {});
  }

  public epochManagerStats(): Promise<GetEpochManagerStatsResponse> {
    return this.transport.sendGet(`epoch-manager/stats`, {});
  }

  public networkStats(): Promise<GetNetworkSyncStateResponse> {
    return this.transport.sendGet(`network/stats`, {});
  }

  public addConnection(req: IndexerAddPeerRequest): Promise<IndexerAddPeerResponse> {
    return this.transport.sendPost(`network/connections`, req);
  }

  public getConnections(): Promise<IndexerGetConnectionsResponse> {
    return this.transport.sendGet(`network/connections`, {});
  }

  public getNonFungibles(params: GetNonFungiblesRequest): Promise<GetNonFungiblesResponse> {
    return this.transport.sendGet(`non-fungibles`, params);
  }

  public substatesGet(id: SubstateId, params: IndexerGetSubstateRequest): Promise<IndexerGetSubstateResponse> {
    return this.transport.sendGet(`substates/${encodeURIComponent(id)}`, params);
  }

  public listSubstates(params: ListSubstatesRequest): Promise<ListSubstatesResponse> {
    return this.transport.sendGet(`substates`, params);
  }

  public fetchSubstates(params: GetSubstatesRequest): Promise<GetSubstatesResponse> {
    return this.transport.sendPost(`substates/fetch`, params);
  }

  public submitTransaction(params: TransactionSubmitRequest): Promise<TransactionSubmitResponse> {
    return this.transport.sendPost(`transactions`, params);
  }

  public getTransactionResult(transaction_id: TransactionId): Promise<IndexerGetTransactionResultResponse> {
    return this.transport.sendGet(`transactions/${encodeURIComponent(transaction_id)}/result`, {});
  }

  public listRecentTransactions(params: ListRecentTransactionsRequest): Promise<ListRecentTransactionsResponse> {
    return this.transport.sendGet(`transactions/recent`, params);
  }

  public listTransactionReceipts(params: ListTransactionReceiptsRequest): Promise<ListTransactionReceiptsResponse> {
    return this.transport.sendGet(`transaction-receipts`, params);
  }

  public getTransactionReceipt(address: TransactionReceiptAddress): Promise<GetTransactionReceiptResponse> {
    return this.transport.sendGet(`transaction-receipts/${address}`, {});
  }

  public templatesGet(template_address: string): Promise<TemplatesGetResponse> {
    return this.transport.sendGet(`templates/${encodeURIComponent(template_address)}`, {});
  }

  public templatesList(limit: number = 0): Promise<ListTemplatesResponse> {
    return this.transport.sendGet(`templates`, { limit });
  }

  public templatesListAuthored(params: TemplatesListAuthoredRequest): Promise<TemplatesListAuthoredResponse> {
    return this.transport.sendPost(`templates`, params);
  }
}
