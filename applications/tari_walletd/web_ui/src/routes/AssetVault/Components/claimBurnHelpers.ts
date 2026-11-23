//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

export type ParsedFeeInput = { kind: "empty" } | { kind: "valid"; value: bigint } | { kind: "invalid" };

/**
 * Parse the optional Max Fee input string (in base units / micro-TARI) without throwing.
 * Empty input means fee estimation should run; otherwise a positive base-10 integer is required.
 */
export function parseFeeInput(raw: string): ParsedFeeInput {
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
