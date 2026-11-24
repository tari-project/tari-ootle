//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { DefaultError } from "@tanstack/react-query";
import { isAuthError } from "@tari-project/wallet_jrpc_client";

export const DEFAULT_QUERY_RETRY_LIMIT = 3;

export function shouldTriggerReauth(error: unknown): boolean {
  if (!isAuthError(error)) {
    return false;
  }
  const method = (error as { cause?: { method?: unknown } }).cause?.method;
  return !(typeof method === "string" && method.startsWith("auth"));
}

export function retryUnlessAuth(maxRetries: number): (failureCount: number, error: DefaultError) => boolean {
  return (failureCount: number, error: DefaultError): boolean => {
    if (isAuthError(error)) {
      return false;
    }
    return failureCount < maxRetries;
  };
}

export const shouldRetryQuery = retryUnlessAuth(DEFAULT_QUERY_RETRY_LIMIT);
