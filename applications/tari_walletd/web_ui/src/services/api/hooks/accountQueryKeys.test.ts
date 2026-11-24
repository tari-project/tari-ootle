//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { accountBalancesQueryKey, isAccountBalancesQueryKey, validatorFeesQueryKey } from "./accountQueryKeys";

const SAMPLE_ACCOUNT = "component_0000000000000000000000000000000000000000000000000000000000000001";

describe("accountQueryKeys", () => {
  describe("isAccountBalancesQueryKey", () => {
    it("matches per-account balance query keys produced by accountBalancesQueryKey", () => {
      expect(isAccountBalancesQueryKey(accountBalancesQueryKey(SAMPLE_ACCOUNT))).toBe(true);
    });

    it("does not match unrelated or bare query keys", () => {
      expect(isAccountBalancesQueryKey(["accounts_balances"])).toBe(false);
      expect(isAccountBalancesQueryKey(["accounts"])).toBe(false);
      expect(isAccountBalancesQueryKey(["accounts_get_default"])).toBe(false);
      expect(isAccountBalancesQueryKey(["transactions"])).toBe(false);
      expect(isAccountBalancesQueryKey([])).toBe(false);
      expect(isAccountBalancesQueryKey([123])).toBe(false);
    });
  });

  describe("validatorFeesQueryKey", () => {
    it("includes accountOrKeyId and shardGroup to avoid cross-validator cache collisions", () => {
      const keyA = validatorFeesQueryKey({ Account: { ComponentAddress: SAMPLE_ACCOUNT } }, null);
      const keyB = validatorFeesQueryKey({ KeyId: { Derived: { key_branch: "account", index: 1n } } }, null);
      const keyC = validatorFeesQueryKey({ Account: { ComponentAddress: SAMPLE_ACCOUNT } }, { start: 0, end: 15 });

      expect(keyA).not.toEqual(keyB);
      expect(keyA).not.toEqual(keyC);
      expect(keyA[0]).toBe("validator_fees");
    });
  });
});
