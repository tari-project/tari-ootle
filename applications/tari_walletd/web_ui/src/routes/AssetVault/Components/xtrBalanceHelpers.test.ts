//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import type { BalanceEntry } from "@tari-project/ootle-ts-bindings";
import { describe, expect, it } from "vitest";
import { formatXtrBalance, XTR_RESOURCE } from "./xtrBalanceHelpers";

function makeEntry(overrides: Partial<BalanceEntry> = {}): BalanceEntry {
  return {
    vault_address: "vault_01",
    resource_address: XTR_RESOURCE,
    balance: "1500000",
    confidential_balance: "500000",
    resource_type: "Stealth",
    token_symbol: "XTR",
    divisibility: 6,
    ...overrides,
  };
}

describe("xtrBalanceHelpers", () => {
  it("formats total and confidential XTR balances and respects loading and hidden states", () => {
    const entries = [makeEntry()];

    expect(formatXtrBalance(entries, { mode: "total", showBalance: true, isLoading: false })).toEqual({
      formattedBalance: "2",
      symbol: "XTR",
    });
    expect(formatXtrBalance(entries, { mode: "confidential", showBalance: true, isLoading: false })).toEqual({
      formattedBalance: "0.500000",
      symbol: "XTR",
    });
    expect(formatXtrBalance(entries, { mode: "total", showBalance: false, isLoading: false })).toEqual({
      formattedBalance: "************",
      symbol: "XTR",
    });
    expect(formatXtrBalance(undefined, { mode: "total", showBalance: true, isLoading: true })).toEqual({
      formattedBalance: "...",
      symbol: "",
    });
  });

  it("falls back to zero balance and empty symbol when no XTR entry is present", () => {
    expect(formatXtrBalance([], { mode: "total", showBalance: true, isLoading: false })).toEqual({
      formattedBalance: "0",
      symbol: "",
    });
    expect(formatXtrBalance([], { mode: "confidential", showBalance: true, isLoading: false })).toEqual({
      formattedBalance: "0",
      symbol: "",
    });
  });

  it("preserves zero divisibility when provided on a balance entry", () => {
    const entries = [makeEntry({ balance: "100", confidential_balance: "25", divisibility: 0 })];

    expect(formatXtrBalance(entries, { mode: "total", showBalance: true, isLoading: false })).toEqual({
      formattedBalance: "125",
      symbol: "XTR",
    });
    expect(formatXtrBalance(entries, { mode: "confidential", showBalance: true, isLoading: false })).toEqual({
      formattedBalance: "25",
      symbol: "XTR",
    });
  });
});
