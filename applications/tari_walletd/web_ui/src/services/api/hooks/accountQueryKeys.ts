//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import type { ComponentAddress } from "@tari-project/ootle-ts-bindings";

export const ACCOUNT_BALANCES_QUERY_KEY = "accounts_balances";

export function accountBalancesQueryKey(account?: ComponentAddress) {
  return [ACCOUNT_BALANCES_QUERY_KEY, account] as const;
}
