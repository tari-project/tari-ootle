/*
 * //  Copyright 2024 The Tari Project
 * //  SPDX-License-Identifier: BSD-3-Clause
 */

import FetchTransport from "./fetch";
import type { SseStream, SseStreamOptions } from "../sse";

export { FetchTransport };

export interface HttpTransport {
  sendGet<T>(path: string, params: any, options?: TransportOptions): Promise<T>;

  sendHead<T>(path: string, params: any, options?: TransportOptions): Promise<T>;

  sendPost<T>(path: string, body: any, options?: TransportOptions): Promise<T>;

  sendPut<T>(path: string, body: any, options?: TransportOptions): Promise<T>;

  sendDelete<T>(path: string, params: Record<string, string>, options?: TransportOptions): Promise<T>;

  sendSse(path: string, params: any, options: SseStreamOptions): SseStream;
}

export interface TransportOptions {
  timeout_millis?: number;
}

