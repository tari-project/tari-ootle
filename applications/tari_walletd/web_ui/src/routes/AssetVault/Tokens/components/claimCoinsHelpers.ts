//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import type { AccountsCreateFreeTestCoinsResponse } from "@tari-project/ootle-ts-bindings";

export function addClaimedAccount(claimedAccounts: readonly string[], accountAddress: string): string[] | null {
  if (claimedAccounts.includes(accountAddress)) {
    return null;
  }
  return [...claimedAccounts, accountAddress];
}

export async function markAccountClaimed(
  targetAddress: string,
  deps: {
    getActiveAccount: () => string | null;
    setHasClaimed: (claimed: boolean) => void;
    getSettings: () => Promise<{ claimed_accounts: string[] }>;
    saveClaimedAccounts: (claimedAccounts: string[]) => Promise<unknown>;
  },
): Promise<void> {
  if (deps.getActiveAccount() === targetAddress) {
    deps.setHasClaimed(true);
  }
  const current = await deps.getSettings();
  const updated = addClaimedAccount(current.claimed_accounts, targetAddress);
  if (!updated) return;
  await deps.saveClaimedAccounts(updated);
}

export function applyClaimSuccess(
  claimedAddress: string,
  resp: AccountsCreateFreeTestCoinsResponse,
  deps: {
    getActiveAccount: () => string | null;
    setAccount: (account: AccountsCreateFreeTestCoinsResponse["account"]) => void;
    setOotleAddress: (address: AccountsCreateFreeTestCoinsResponse["address"]) => void;
  },
): void {
  if (deps.getActiveAccount() === claimedAddress) {
    deps.setAccount(resp.account);
    deps.setOotleAddress(resp.address);
  }
}
