//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import type { BalanceEntry } from "@tari-project/ootle-ts-bindings";
import { bigintToDecimalString } from "@utils/helpers";

export const XTR_RESOURCE = "resource_0101010101010101010101010101010101010101010101010101010101010101";

export function parseBalanceBigInt(value: string | number | bigint | null | undefined): bigint {
  if (value === null || value === undefined || value === "") {
    return 0n;
  }
  if (typeof value === "bigint") {
    return value;
  }
  try {
    return BigInt(value);
  } catch {
    return 0n;
  }
}

export function formatXtrBalance(
  balances: BalanceEntry[] | undefined,
  {
    mode,
    showBalance,
    isLoading,
  }: {
    mode: "total" | "confidential";
    showBalance: boolean;
    isLoading: boolean;
  },
): { formattedBalance: string; symbol: string } {
  const balanceObj = balances?.find((b) => b.resource_address === XTR_RESOURCE);
  const symbol = balanceObj?.token_symbol || "";

  if (isLoading && !balances) {
    return { formattedBalance: "...", symbol };
  }

  if (!showBalance) {
    return { formattedBalance: "************", symbol };
  }

  const confidential = parseBalanceBigInt(balanceObj?.confidential_balance);
  const amount = mode === "total" ? parseBalanceBigInt(balanceObj?.balance) + confidential : confidential;
  const decimals = balanceObj?.divisibility ?? 6;

  return {
    formattedBalance: bigintToDecimalString(amount, decimals),
    symbol,
  };
}
