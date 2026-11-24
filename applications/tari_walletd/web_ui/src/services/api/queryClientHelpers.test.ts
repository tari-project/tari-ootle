//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { shouldRetryQuery, shouldTriggerReauth } from "./queryClientHelpers";

function makeRpcError(code: number, method?: unknown): Error {
  return new Error(`RPC Error ${code}`, {
    cause: method === undefined ? { code } : { code, method },
  });
}

describe("queryClientHelpers", () => {
  it("triggers reauth for non-auth 401 errors and skips auth.* methods or non-401 errors", () => {
    expect(shouldTriggerReauth(makeRpcError(401, "accounts.list"))).toBe(true);
    expect(shouldTriggerReauth(makeRpcError(401))).toBe(true);
    expect(shouldTriggerReauth(makeRpcError(401, "auth.request"))).toBe(false);
    expect(shouldTriggerReauth(makeRpcError(500, "accounts.list"))).toBe(false);
    expect(shouldTriggerReauth(new Error("network down"))).toBe(false);
  });

  it("does not retry 401 auth errors and retries other errors up to 3 times", () => {
    expect(shouldRetryQuery(0, makeRpcError(401, "accounts.list"))).toBe(false);
    expect(shouldRetryQuery(0, makeRpcError(500, "accounts.list"))).toBe(true);
    expect(shouldRetryQuery(2, makeRpcError(500, "accounts.list"))).toBe(true);
    expect(shouldRetryQuery(3, makeRpcError(500, "accounts.list"))).toBe(false);
  });
});
