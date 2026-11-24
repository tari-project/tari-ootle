//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

export function isAccountBalanceQueryKey(queryKey: readonly unknown[]): boolean {
  const key = queryKey[0];
  return typeof key === "string" && key.startsWith("accounts_balances_");
}

export function addClaimedAccount(claimedAccounts: readonly string[], accountAddress: string): string[] | null {
  if (claimedAccounts.includes(accountAddress)) {
    return null;
  }
  return [...claimedAccounts, accountAddress];
}
