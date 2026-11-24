//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { isAuthError } from "@tari-project/wallet_jrpc_client";

export const DEFAULT_QUERY_RETRY_LIMIT = 3;

export function shouldTriggerReauth(error: unknown): boolean {
  if (!isAuthError(error)) {
    return false;
  }
  const method = (error as { cause?: { method?: unknown } }).cause?.method;
  return !(typeof method === "string" && method.startsWith("auth"));
}

export function shouldRetryQuery(failureCount: number, error: unknown): boolean {
  if (isAuthError(error)) {
    return false;
  }
  return failureCount < DEFAULT_QUERY_RETRY_LIMIT;
}
