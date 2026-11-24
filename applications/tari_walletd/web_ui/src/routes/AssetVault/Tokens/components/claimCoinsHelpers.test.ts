//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { addClaimedAccount, isAccountBalanceQueryKey } from "./claimCoinsHelpers";

describe("claimCoinsHelpers", () => {
  describe("isAccountBalanceQueryKey", () => {
    it("matches per-account balance query keys used by useAccountsGetBalances", () => {
      expect(
        isAccountBalanceQueryKey([
          "accounts_balances_component_0000000000000000000000000000000000000000000000000000000000000001",
        ]),
      ).toBe(true);
    });

    it("does not match unrelated query keys", () => {
      expect(isAccountBalanceQueryKey(["accounts_get_balances_component_1"])).toBe(false);
      expect(isAccountBalanceQueryKey(["accounts"])).toBe(false);
      expect(isAccountBalanceQueryKey(["faucet_balance"])).toBe(false);
      expect(isAccountBalanceQueryKey([])).toBe(false);
    });
  });

  describe("addClaimedAccount", () => {
    it("appends a newly claimed account without mutating the input array", () => {
      const existing = ["component_a"];
      const updated = addClaimedAccount(existing, "component_b");
      expect(updated).toEqual(["component_a", "component_b"]);
      expect(existing).toEqual(["component_a"]);
    });

    it("returns null when the account is already recorded as claimed", () => {
      expect(addClaimedAccount(["component_a", "component_b"], "component_a")).toBeNull();
    });
  });
});
