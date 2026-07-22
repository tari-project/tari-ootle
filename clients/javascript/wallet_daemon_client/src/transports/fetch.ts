/*
 * //  Copyright 2024 The Tari Project
 * //  SPDX-License-Identifier: BSD-3-Clause
 */

import { RpcRequest, RpcResponse, RpcTransport, RpcTransportOptions } from "./index";

export interface FetchRpcTransportOptions {
  /**
   * Cookie policy applied to every request this transport makes, matching the
   * `credentials` option of `fetch`. Defaults to `"same-origin"`.
   *
   * The daemon's refresh grant lives in an HttpOnly, `SameSite=Strict` cookie
   * issued by `auth.request`, so only a same-origin browser caller can hold a
   * refreshable session. A cross-origin caller — a browser extension, or a
   * page served from another host — cannot obtain that cookie under any
   * setting here, and must authenticate with an API key instead.
   */
  credentials?: RequestCredentials;
}

export default class FetchRpcTransport implements RpcTransport {
  private url: string;
  private credentials: RequestCredentials;

  constructor(url: string, options?: FetchRpcTransportOptions) {
    this.url = url;
    this.credentials = options?.credentials ?? "same-origin";
  }

  static new(url: string, options?: FetchRpcTransportOptions) {
    return new FetchRpcTransport(url, options);
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
      credentials: this.credentials,
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
