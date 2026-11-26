// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

/** True when a JSON-RPC call failed because the requested account does not exist (`error.cause.code` is 404). */
export function isAccountNotFoundError(error: unknown): boolean {
  if (!error || typeof error !== "object") {
    return false;
  }
  const cause = (error as { cause?: unknown }).cause;
  return Boolean(cause && typeof cause === "object" && (cause as { code?: unknown }).code === 404);
}
