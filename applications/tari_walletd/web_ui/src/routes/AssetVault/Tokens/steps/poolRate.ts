//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { ResourceAddress, SwapPoolInfo, TARI_TOKEN } from "@tari-project/ootle-ts-bindings";
import { XTR_CURRENCY } from "@utils/currency";

export interface PoolRateInfo {
  resource_a: string;
  balance_a: bigint;
  resource_b: string;
  balance_b: bigint;
}

/**
 * Formats a non-negative rational `numerator / denominator` to `fractionDigits`
 * decimal places using BigInt half-up rounding, avoiding IEEE-754 precision loss
 * on large pool reserves.
 */
export function formatScaledRatio(numerator: bigint, denominator: bigint, fractionDigits: number): string {
  const digits = Math.max(0, Math.trunc(fractionDigits));
  if (denominator <= 0n || numerator <= 0n) {
    return digits === 0 ? "0" : `0.${"0".repeat(digits)}`;
  }

  const scale = 10n ** BigInt(digits);
  const scaled = (numerator * scale * 2n + denominator) / (denominator * 2n);
  if (digits === 0) {
    return scaled.toString();
  }

  const whole = scaled / scale;
  const frac = (scaled % scale).toString().padStart(digits, "0");
  return `${whole}.${frac}`;
}

/**
 * Formats the exchange rate of 1 whole token in whole TARI given base-unit pool
 * reserves and the token's divisibility.
 */
export function formatTariPerTokenRate(
  tariBalance: bigint,
  tokenBalance: bigint,
  tokenDivisibility: number,
  fractionDigits: number,
): string {
  const decimals = Math.max(0, Math.trunc(tokenDivisibility));
  const diff = decimals - XTR_CURRENCY.decimals;
  const numerator = diff >= 0 ? tariBalance * 10n ** BigInt(diff) : tariBalance;
  const denominator = diff >= 0 ? tokenBalance : tokenBalance * 10n ** BigInt(-diff);
  return formatScaledRatio(numerator, denominator, fractionDigits);
}

/**
 * Keeps only pools that pair `resourceAddress` with `TARI_TOKEN`, since fee
 * swaps must output TARI.
 */
export function filterTariSwapPools(pools: SwapPoolInfo[], resourceAddress?: ResourceAddress): SwapPoolInfo[] {
  if (!resourceAddress || resourceAddress === TARI_TOKEN) {
    return [];
  }
  return pools.filter(
    (p) =>
      (p.resource_a === resourceAddress && p.resource_b === TARI_TOKEN) ||
      (p.resource_b === resourceAddress && p.resource_a === TARI_TOKEN),
  );
}

export function formatPoolRate(poolRate: PoolRateInfo, tokenSymbol: string, tokenDivisibility: number): string {
  const a = poolRate.balance_a;
  const b = poolRate.balance_b;
  if (a <= 0n || b <= 0n) {
    return "Pool has no liquidity";
  }

  if (poolRate.resource_a === TARI_TOKEN) {
    return `1 ${tokenSymbol} = ${formatTariPerTokenRate(a, b, tokenDivisibility, 6)} TARI`;
  }
  if (poolRate.resource_b === TARI_TOKEN) {
    return `1 ${tokenSymbol} = ${formatTariPerTokenRate(b, a, tokenDivisibility, 6)} TARI`;
  }
  return `1:${formatScaledRatio(a, b, 6)}`;
}
