//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import {
  DEFAULT_QUERY_RETRY_LIMIT,
  retryUnlessAuth,
  shouldRetryQuery,
  shouldTriggerReauth,
} from "./queryClientHelpers";

function makeError(code: number, method?: unknown): Error {
  return new Error("RPC error", { cause: { code, method } });
}

describe("shouldTriggerReauth", () => {
  it("triggers reauth for non-auth 401 errors and skips auth.* methods or non-401 errors", () => {
    expect(shouldTriggerReauth(makeError(401, "accounts.list"))).toBe(true);
    expect(shouldTriggerReauth(makeError(401, "auth.refresh"))).toBe(false);
    expect(shouldTriggerReauth(makeError(401, "auth.request"))).toBe(false);
    expect(shouldTriggerReauth(makeError(401))).toBe(true);
    expect(shouldTriggerReauth(makeError(500, "accounts.list"))).toBe(false);
    expect(shouldTriggerReauth(new Error("network error"))).toBe(false);
  });
});

describe("shouldRetryQuery and retryUnlessAuth", () => {
  it("never retries 401 auth errors and respects the configured retry limit for other errors", () => {
    const authErr = makeError(401, "accounts.list");
    const transientErr = makeError(500, "accounts.list");

    expect(shouldRetryQuery(0, authErr)).toBe(false);
    expect(shouldRetryQuery(0, transientErr)).toBe(true);
    expect(shouldRetryQuery(DEFAULT_QUERY_RETRY_LIMIT - 1, transientErr)).toBe(true);
    expect(shouldRetryQuery(DEFAULT_QUERY_RETRY_LIMIT, transientErr)).toBe(false);

    const retryTwice = retryUnlessAuth(2);
    expect(retryTwice(0, authErr)).toBe(false);
    expect(retryTwice(0, transientErr)).toBe(true);
    expect(retryTwice(1, transientErr)).toBe(true);
    expect(retryTwice(2, transientErr)).toBe(false);
  });
});
