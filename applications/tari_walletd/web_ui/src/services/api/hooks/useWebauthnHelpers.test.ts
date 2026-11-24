// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it, vi } from "vitest";
import { invalidatePasskeysList, WEBAUTHN_PASSKEYS_QUERY_KEY } from "./useWebauthnHelpers";

describe("invalidatePasskeysList", () => {
  it("returns the invalidateQueries promise targeting WEBAUTHN_PASSKEYS_QUERY_KEY", async () => {
    let resolved = false;
    const invalidateQueries = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          setTimeout(() => {
            resolved = true;
            resolve();
          }, 0);
        }),
    );

    const promise = invalidatePasskeysList({ invalidateQueries });
    expect(invalidateQueries).toHaveBeenCalledWith({ queryKey: WEBAUTHN_PASSKEYS_QUERY_KEY });
    expect(resolved).toBe(false);
    await promise;
    expect(resolved).toBe(true);
  });
});
