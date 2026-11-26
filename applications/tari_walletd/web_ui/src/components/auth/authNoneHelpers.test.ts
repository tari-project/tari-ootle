//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it, vi } from "vitest";
import { normalizeAuthError, runAuthNone } from "./authNoneHelpers";

describe("runAuthNone", () => {
  it("sets token and invokes onAuthenticated when still active", async () => {
    const setToken = vi.fn();
    const authRequest = vi.fn().mockResolvedValue("jwt-token");
    const onAuthenticated = vi.fn();
    const onError = vi.fn();

    await runAuthNone({
      getClient: async () => ({ authRequest, setToken }),
      permissions: ["Admin"],
      isCancelled: () => false,
      onAuthenticated,
      onError,
    });

    expect(authRequest).toHaveBeenCalledWith(["Admin"], "None");
    expect(setToken).toHaveBeenCalledWith("jwt-token");
    expect(onAuthenticated).toHaveBeenCalledTimes(1);
    expect(onError).not.toHaveBeenCalled();
  });

  it("ignores stale success and error results after cancellation", async () => {
    let cancelled = false;
    const setToken = vi.fn();
    const onAuthenticated = vi.fn();
    const onError = vi.fn();

    await runAuthNone({
      getClient: async () => ({
        authRequest: async () => {
          cancelled = true;
          return "stale-token";
        },
        setToken,
      }),
      permissions: ["Admin"],
      isCancelled: () => cancelled,
      onAuthenticated,
      onError,
    });

    expect(setToken).not.toHaveBeenCalled();
    expect(onAuthenticated).not.toHaveBeenCalled();
    expect(onError).not.toHaveBeenCalled();

    let cancelledErr = false;
    await runAuthNone({
      getClient: async () => ({
        authRequest: async () => {
          cancelledErr = true;
          throw new Error("stale failure");
        },
        setToken,
      }),
      permissions: ["Admin"],
      isCancelled: () => cancelledErr,
      onAuthenticated,
      onError,
    });

    expect(onError).not.toHaveBeenCalled();
  });

  it("normalizes non-Error failures and reports them via onError", async () => {
    const onError = vi.fn();

    await runAuthNone({
      getClient: async () => ({
        authRequest: async () => {
          throw null;
        },
        setToken: vi.fn(),
      }),
      permissions: ["Admin"],
      isCancelled: () => false,
      onAuthenticated: vi.fn(),
      onError,
    });

    expect(onError).toHaveBeenCalledTimes(1);
    expect(onError.mock.calls[0][0]).toBeInstanceOf(Error);
    expect(onError.mock.calls[0][0].message).toBe("Unknown authentication error");
    expect(normalizeAuthError("boom").message).toBe("boom");
  });
});
