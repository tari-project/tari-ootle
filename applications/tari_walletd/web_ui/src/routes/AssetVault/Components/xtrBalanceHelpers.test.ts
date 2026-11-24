//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import type { BalanceEntry } from "@tari-project/ootle-ts-bindings";
import { describe, expect, it } from "vitest";
import { formatXtrBalance, parseBalanceBigInt, XTR_RESOURCE } from "./xtrBalanceHelpers";

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
  it("parses valid balance values and falls back to 0n on malformed inputs", () => {
    expect(parseBalanceBigInt("123456")).toBe(123456n);
    expect(parseBalanceBigInt(42)).toBe(42n);
    expect(parseBalanceBigInt(99n)).toBe(99n);
    expect(parseBalanceBigInt("")).toBe(0n);
    expect(parseBalanceBigInt(null)).toBe(0n);
    expect(parseBalanceBigInt("not-a-number")).toBe(0n);
  });

  it("preserves zero divisibility instead of defaulting to 6 decimals", () => {
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

  it("formats total and confidential balances with default 6 decimals and respects loading/hidden states", () => {
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
});
