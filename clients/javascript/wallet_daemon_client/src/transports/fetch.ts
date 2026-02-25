/*
 * //  Copyright 2024 The Tari Project
 * //  SPDX-License-Identifier: BSD-3-Clause
 */

import { RpcRequest, RpcResponse, RpcTransport, RpcTransportOptions } from "./index";

export default class FetchRpcTransport implements RpcTransport {
  private url: string;

  constructor(url: string) {
    this.url = url;
  }

  static new(url: string) {
    return new FetchRpcTransport(url);
  }

  async sendRequest<T>(data: RpcRequest, options?: RpcTransportOptions): Promise<RpcResponse<T>> {
    const headers = {
      "Content-Type": "application/json",
    };
    if (options?.token) {
      headers["Authorization"] = `Bearer ${options.token}`;
    }

    let controller = new AbortController();
    let signal = controller.signal;

    const timeoutId = options?.timeout_millis
      ? setTimeout(() => {
          controller.abort("Timeout");
        }, options.timeout_millis)
      : null;

    const response = await fetch(this.url, {
      method: "POST",
      body: JSON.stringify(data),
      headers,
      signal,
    });
    if (timeoutId) {
      clearTimeout(timeoutId);
    }

    // HTTP errors are handled in the transport layer
    if (!response.ok) {
      const errorText = await response.text();
      throw new Error(`HTTP error ${response.status}: ${errorText}`);
    }

    let resp = await response.json();
    return resp;
  }
}
