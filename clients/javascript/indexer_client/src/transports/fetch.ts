/*
 * //  Copyright 2024 The Tari Project
 * //  SPDX-License-Identifier: BSD-3-Clause
 */

import { HttpTransport, TransportOptions } from "./index";

export default class FetchTransport implements HttpTransport {
  private url: string;

  constructor(url: string) {
    this.url = url;
  }

  static new(url: string) {
    if (!url.startsWith("http://") && !url.startsWith("https://")) {
      throw new Error("URL must start with http:// or https://");
    }
    if (url.endsWith("/")) {
      url = url.slice(0, -1);
    }
    return new FetchTransport(url);
  }

  sendGet<T>(path: string, params: any, options?: TransportOptions): Promise<T> {
    return this.sendRequest<T>(path, { method: "GET", params }, options);
  }

  sendHead<T>(path: string, params: any, options?: TransportOptions): Promise<T> {
    return this.sendRequest<T>(path, { method: "HEAD", params }, options);
  }

  sendPost<T>(path: string, body: any, options?: TransportOptions): Promise<T> {
    return this.sendRequest<T>(path, { method: "POST", body }, options);
  }

  sendPut<T>(path: string, body: any, options?: TransportOptions): Promise<T> {
    return this.sendRequest<T>(path, { method: "PUT", body }, options);
  }

  sendDelete<T>(path: string, body: any, options?: TransportOptions): Promise<T> {
    return this.sendRequest<T>(path, { method: "DELETE", body }, options);
  }

  async sendRequest<T>(path: string, request: RequestSpec, options?: TransportOptions): Promise<T> {
    let controller = new AbortController();
    let signal = controller.signal;

    const timeoutId = options?.timeout_millis
      ? setTimeout(() => {
        controller.abort("Timeout");
      }, options.timeout_millis)
      : null;

    if (path.startsWith("/")) {
      path = path.slice(1);
    }

    let query = null;
    if (typeof request.params === "object" && request.params !== null) {
      const urlParams = new URLSearchParams();
      for (const [key, value] of Object.entries(request.params)) {
        if (value !== undefined && value !== null) {
          urlParams.append(key, value.toString());
        }
      }
      query = urlParams;
    }
    if (typeof request.body === "object" && request.body !== null) {
      // Add content-type header
      if (!request.headers) {
        request.headers = {};
      }
      request.headers["Content-Type"] = "application/json";
      request.body = JSON.stringify(request.body);
    }

    let fqPath = `${this.url}/${path}`;
    if (query && query.toString().length > 0) {
      fqPath += `?${query.toString()}`;
    }

    const response = await fetch(fqPath, {
      signal,
      ...request,
    });
    if (timeoutId) {
      clearTimeout(timeoutId);
    }
    if (!response.ok) {
      const text = await response.text();
      throw new Error(`HTTP ${response.status}: ${response.statusText}${text ? ` - ${text}` : ""}`);
    }
    const json = await response.json();
    if (json.error) {
      throw new Error(`${json.error.code}: ${json.error.message}`);
    }
    return json;
  }
}

export interface RequestSpec {
  method: "GET" | "HEAD" | "POST" | "PUT" | "DELETE";
  headers?: Record<string, string>;
  params?: any;
  body?: any;
}
