/*
 * //  Copyright 2024 The Tari Project
 * //  SPDX-License-Identifier: BSD-3-Clause
 */

import "./serialize";
import type {
  Event,
  GetEpochManagerStatsResponse,
  GetNetworkSyncStateResponse,
  GetNonFungiblesRequest,
  GetNonFungiblesResponse,
  GetResourceResponse,
  ResourceAddress,
  GetSubstatesRequest,
  GetSubstatesResponse,
  GetTransactionReceiptResponse,
  IndexerGetConnectionsResponse,
  IndexerGetIdentityResponse,
  IndexerGetSubstateRequest,
  IndexerGetSubstateResponse,
  IndexerGetTransactionResultResponse,
  IndexerReadyResponse,
  ListRecentTransactionsRequest,
  ListRecentTransactionsResponse,
  ListTemplatesResponse,
  ListTransactionReceiptsRequest,
  ListTransactionReceiptsResponse,
  rejectReasonToString,
  stringToSubstateId,
  StreamTransactionEventsRequest,
  SubstateId,
  substateIdToString,
  TemplatesGetResponse,
  TemplatesListAuthoredRequest,
  TemplatesListAuthoredResponse,
  TransactionId,
  TransactionReceiptAddress,
  TransactionSubmitRequest,
  TransactionSubmitResponse,
  QueryTransactionEventsRequest,
  QueryTransactionEventsResponse,
  GetNetworkInfoResponse,
  ListWatchedSubstatesRequest,
  ListWatchedSubstatesResponse,
  ListWatchedTemplatesResponse,
} from "@tari-project/ootle-ts-bindings";
import { FetchTransport, HttpTransport } from "./transports";
import type { SseStream } from "./sse";

export * as transports from "./transports";
export type { SseEvent, SseStream, SseStreamOptions } from "./sse";

export { substateIdToString, stringToSubstateId, rejectReasonToString };

/**
 * A template-emitted event with its originating transaction ID.
 * Streamed via the /transactions/events/stream SSE endpoint.
 */
export interface TransactionEvent {
  id: number;
  transaction_id: TransactionId;
  event: Event;
}

export interface TransactionEventStreamOptions {
  /** Called for each received transaction event */
  onEvent: (event: TransactionEvent) => void;
  /** Called when the stream encounters an error */
  onError?: (error: Error) => void;
  /** Called when the stream closes */
  onClose?: () => void;
  /** AbortSignal to cancel the stream */
  signal?: AbortSignal;
}

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
  public networkInfo(): Promise<GetNetworkInfoResponse> {
    return this.transport.sendGet(`network`, {});
  }

  public networkStats(): Promise<GetNetworkSyncStateResponse> {
    return this.transport.sendGet(`network/stats`, {});
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
  public queryTransactionEvents(params: QueryTransactionEventsRequest): Promise<QueryTransactionEventsResponse> {
    return this.transport.sendGet(`transactions/events`, params);
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

  public templatesListCached(limit: number = 0): Promise<ListTemplatesResponse> {
    return this.transport.sendGet(`templates/cached`, { limit });
  }

  public resourcesGet(address: ResourceAddress): Promise<GetResourceResponse> {
    return this.transport.sendGet(`resources/${encodeURIComponent(address)}`, {});
  }

  public resourceGetTari(): Promise<GetResourceResponse> {
    return this.transport.sendGet(`resources/tari`, {});
  }

  public listWatchedTemplates(): Promise<ListWatchedTemplatesResponse> {
    return this.transport.sendGet(`templates/watched`, {});
  }

  public listWatchedSubstates(params: Partial<ListWatchedSubstatesRequest>): Promise<ListWatchedSubstatesResponse> {
    return this.transport.sendGet(`substates/watched`, params);
  }

  /**
   * Subscribe to a filtered stream of template-emitted transaction events via SSE.
   *
   * Returns an `SseStream` handle — call `.close()` to disconnect.
   */
  public streamTransactionEvents(
    params: Partial<StreamTransactionEventsRequest>,
    options: TransactionEventStreamOptions,
  ): SseStream {
    return this.transport.sendSse(`transactions/events/stream`, params, {
      onEvent(sseEvent) {
        let parsed: TransactionEvent;
        try {
          parsed = JSON.parse(sseEvent.data);
        } catch (e) {
          options.onError?.(new Error(`Failed to parse TransactionEvent: ${e}`));
          return;
        }
        // The event ID is transmitted via the SSE id: field, not in the JSON payload.
        if (sseEvent.id) {
          parsed.id = parseInt(sseEvent.id, 10);
        }
        options.onEvent(parsed);
      },
      onError: options.onError,
      onClose: options.onClose,
      signal: options.signal,
    });
  }
}
