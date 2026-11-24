//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { partialMatchKey } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import { ACCOUNT_BALANCES_QUERY_KEY, accountBalancesQueryKey } from "./accountQueryKeys";

const ACCOUNT_A = "component_0000000000000000000000000000000000000000000000000000000000000001";
const ACCOUNT_B = "component_0000000000000000000000000000000000000000000000000000000000000002";

describe("accountBalancesQueryKey", () => {
  it("matches the shared accounts_balances prefix used by mutation invalidations", () => {
    const key = accountBalancesQueryKey(ACCOUNT_A);
    expect(key[0]).toBe(ACCOUNT_BALANCES_QUERY_KEY);
    expect(partialMatchKey(key, [ACCOUNT_BALANCES_QUERY_KEY])).toBe(true);
  });

  it("isolates per-account balance invalidations", () => {
    const keyA = accountBalancesQueryKey(ACCOUNT_A);
    const keyB = accountBalancesQueryKey(ACCOUNT_B);

    expect(partialMatchKey(keyA, keyA)).toBe(true);
    expect(partialMatchKey(keyA, keyB)).toBe(false);
    expect(partialMatchKey(["accounts"], [ACCOUNT_BALANCES_QUERY_KEY])).toBe(false);
  });
});
