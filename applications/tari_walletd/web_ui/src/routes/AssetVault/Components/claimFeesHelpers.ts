//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import type { AccountInfo, FeePoolDetails } from "@tari-project/ootle-ts-bindings";
import { substateIdToString } from "@tari-project/ootle-ts-bindings";

export interface ClaimKey {
  index: bigint;
  account: AccountInfo["account"];
}

export type ParsedOptionalFee = { kind: "empty" } | { kind: "valid"; value: bigint } | { kind: "invalid" };

/**
 * Extract derived account keys eligible for claiming validator fees.
 */
export function getAccountClaimKeys(accounts: readonly AccountInfo[] | undefined): ClaimKey[] {
  if (!accounts) {
    return [];
  }
  return accounts.flatMap((info): ClaimKey[] => {
    const keyId = info.account.owner_key_id;
    if (keyId !== null && "Derived" in keyId && keyId.Derived.key_branch === "account") {
      return [{ index: BigInt(keyId.Derived.index), account: info.account }];
    }
    return [];
  });
}

/**
 * Find the component address of the account whose derived account key matches `keyIndex`.
 */
export function findAccountAddressForClaimKey(claimKeys: readonly ClaimKey[], keyIndex: number): string | null {
  const targetIndex = BigInt(keyIndex);
  const match = claimKeys.find((entry) => entry.index === targetIndex);
  return match?.account.component_address ? substateIdToString(match.account.component_address) : null;
}

/**
 * Sum the scanned validator fee pool amounts (in base units) across all shards.
 */
export function sumScannedFees(fees: Record<string, FeePoolDetails | undefined>): bigint {
  return Object.values(fees).reduce((acc, info) => (info ? acc + BigInt(info.amount) : acc), 0n);
}

/**
 * Parse the optional Max Fee field (in base units).
 * Empty/whitespace means dry-run fee estimation; otherwise a positive integer is required.
 */
export function parseOptionalFee(raw: string): ParsedOptionalFee {
  const trimmed = raw.trim();
  if (trimmed === "") {
    return { kind: "empty" };
  }
  if (!/^\d+$/.test(trimmed)) {
    return { kind: "invalid" };
  }
  const value = BigInt(trimmed);
  if (value <= 0n) {
    return { kind: "invalid" };
  }
  return { kind: "valid", value };
}

export function canSubmitClaimFees(keyIndex: number | null, shards: readonly number[], rawFee: string): boolean {
  return keyIndex !== null && shards.length > 0 && parseOptionalFee(rawFee).kind !== "invalid";
}
