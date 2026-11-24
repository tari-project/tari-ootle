//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { Account, AccountInfo, ComponentAddress, substateIdToString } from "@tari-project/ootle-ts-bindings";

export interface AccountSelectOption {
  address: ComponentAddress;
  label: string;
}

export function buildAccountSelectOptions(
  accounts: readonly AccountInfo[] | undefined,
  activeAccount: Account | null | undefined,
): AccountSelectOption[] {
  const options: AccountSelectOption[] = (accounts ?? []).map((info) => ({
    address: substateIdToString(info.account.component_address),
    label: info.account.name || "<No Name>",
  }));

  if (activeAccount) {
    const activeAddress = substateIdToString(activeAccount.component_address);
    if (!options.some((opt) => opt.address === activeAddress)) {
      options.push({
        address: activeAddress,
        label: activeAccount.name || "<No Name>",
      });
    }
  }

  return options;
}

export function resolveSelectedAccountValue(
  options: readonly AccountSelectOption[],
  activeAccount: Account | null | undefined,
): string {
  return activeAccount ? substateIdToString(activeAccount.component_address) : (options[0]?.address ?? "");
}
