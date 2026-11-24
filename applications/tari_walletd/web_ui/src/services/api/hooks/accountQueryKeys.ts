//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import type { AccountOrKeyId, ComponentAddress } from "@tari-project/ootle-ts-bindings";

export const ACCOUNT_BALANCES_KEY_PREFIX = "accounts_balances_";

export function accountBalancesQueryKey(account?: ComponentAddress): [string] {
  return [`${ACCOUNT_BALANCES_KEY_PREFIX}${account}`];
}

export function isAccountBalancesQueryKey(queryKey: readonly unknown[]): boolean {
  const first = queryKey[0];
  return typeof first === "string" && first.startsWith(ACCOUNT_BALANCES_KEY_PREFIX);
}

export function validatorFeesQueryKey(accountOrKeyId: AccountOrKeyId, shardGroup: unknown = null) {
  return ["validator_fees", accountOrKeyId, shardGroup] as const;
}
