//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import type { BalanceEntry } from "@tari-project/ootle-ts-bindings";
import { bigintToDecimalString } from "@utils/helpers";

export const XTR_RESOURCE = "resource_0101010101010101010101010101010101010101010101010101010101010101";

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

  const confidential = BigInt(balanceObj?.confidential_balance ?? 0);
  const amount = mode === "total" ? BigInt(balanceObj?.balance ?? 0) + confidential : confidential;
  const decimals = balanceObj?.divisibility ?? 6;

  return {
    formattedBalance: bigintToDecimalString(amount, decimals),
    symbol,
  };
}
