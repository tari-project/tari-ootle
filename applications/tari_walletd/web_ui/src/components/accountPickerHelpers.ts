// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { Account, AccountInfo, substateIdToString } from "@tari-project/ootle-ts-bindings";

export interface AccountPickerOption {
  address: string;
  label: string;
}

export function resolveFallbackAccount(accounts: readonly AccountInfo[] | undefined): AccountInfo | undefined {
  if (!accounts || accounts.length === 0) {
    return undefined;
  }
  return accounts.find((entry) => entry.account.is_default) ?? accounts[0];
}

export function buildAccountPickerOptions(
  accounts: readonly AccountInfo[] | undefined,
  currentAccount: Account | undefined,
): AccountPickerOption[] {
  const options: AccountPickerOption[] = [];
  const seen = new Set<string>();

  for (const entry of accounts ?? []) {
    const address = substateIdToString(entry.account.component_address);
    if (seen.has(address)) {
      continue;
    }
    seen.add(address);
    options.push({
      address,
      label: entry.account.name || address,
    });
  }

  if (currentAccount) {
    const currentAddress = substateIdToString(currentAccount.component_address);
    if (!seen.has(currentAddress)) {
      options.unshift({
        address: currentAddress,
        label: currentAccount.name || currentAddress,
      });
    }
  }

  return options;
}
