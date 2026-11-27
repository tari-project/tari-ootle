//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import type { ResourceAddress, SwapPoolInfo } from "@tari-project/ootle-ts-bindings";
import { TARI_TOKEN } from "@tari-project/ootle-ts-bindings";
import { describe, expect, it } from "vitest";
import { filterTariSwapPools, formatPoolRate, formatScaledRatio, formatTariPerTokenRate } from "./poolRate";

const TOKEN_A = `resource_${"11".repeat(32)}` as ResourceAddress;
const TOKEN_B = `resource_${"22".repeat(32)}` as ResourceAddress;

describe("formatScaledRatio", () => {
  it("rounds half-up at the requested decimal precision", () => {
    expect(formatScaledRatio(1n, 3n, 6)).toBe("0.333333");
    expect(formatScaledRatio(2n, 3n, 6)).toBe("0.666667");
    expect(formatScaledRatio(1n, 2n, 0)).toBe("1");
  });

  it("handles zero or non-positive inputs without throwing", () => {
    expect(formatScaledRatio(0n, 10n, 4)).toBe("0.0000");
    expect(formatScaledRatio(10n, 0n, 4)).toBe("0.0000");
  });

  it("preserves exactness for reserves exceeding Number.MAX_SAFE_INTEGER", () => {
    const huge = 10n ** 30n;
    expect(formatScaledRatio(huge * 3n, huge * 2n, 6)).toBe("1.500000");
  });
});

describe("formatTariPerTokenRate", () => {
  it("formats 1:1 parity when token divisibility matches TARI (6)", () => {
    expect(formatTariPerTokenRate(100_000_000n, 100_000_000n, 6, 6)).toBe("1.000000");
  });

  it("adjusts for the default fungible token divisibility (8) vs TARI (6)", () => {
    // 100 TARI (6 decimals) and 100 TOKEN (8 decimals) => 1 TOKEN = 1 TARI.
    expect(formatTariPerTokenRate(100_000_000n, 10_000_000_000n, 8, 6)).toBe("1.000000");
    expect(formatTariPerTokenRate(100_000_000n, 10_000_000_000n, 8, 4)).toBe("1.0000");
  });

  it("adjusts for whole-unit tokens with divisibility 0", () => {
    // 50 TARI (6 decimals) and 25 whole TOKEN (0 decimals) => 1 TOKEN = 2 TARI.
    expect(formatTariPerTokenRate(50_000_000n, 25n, 0, 6)).toBe("2.000000");
  });

  it("adjusts for high-divisibility (18) tokens without underflowing to zero", () => {
    // 2.5 TARI (6 decimals) and 1 TOKEN (18 decimals) => 1 TOKEN = 2.5 TARI.
    expect(formatTariPerTokenRate(2_500_000n, 10n ** 18n, 18, 6)).toBe("2.500000");
  });
});

describe("formatPoolRate", () => {
  it("formats rates regardless of whether TARI is resource_a or resource_b", () => {
    expect(
      formatPoolRate(
        {
          resource_a: TARI_TOKEN,
          balance_a: 200_000_000n,
          resource_b: TOKEN_A,
          balance_b: 10_000_000_000n,
        },
        "TOK",
        8,
      ),
    ).toBe("1 TOK = 2.000000 TARI");

    expect(
      formatPoolRate(
        {
          resource_a: TOKEN_A,
          balance_a: 10_000_000_000n,
          resource_b: TARI_TOKEN,
          balance_b: 200_000_000n,
        },
        "TOK",
        8,
      ),
    ).toBe("1 TOK = 2.000000 TARI");
  });

  it("reports no liquidity when either reserve is zero", () => {
    expect(
      formatPoolRate(
        {
          resource_a: TARI_TOKEN,
          balance_a: 0n,
          resource_b: TOKEN_A,
          balance_b: 100n,
        },
        "TOK",
        8,
      ),
    ).toBe("Pool has no liquidity");
  });

  it("falls back to raw ratio when neither resource is TARI", () => {
    expect(
      formatPoolRate(
        {
          resource_a: TOKEN_A,
          balance_a: 3n,
          resource_b: TOKEN_B,
          balance_b: 2n,
        },
        "TOK",
        8,
      ),
    ).toBe("1:1.500000");
  });
});

describe("filterTariSwapPools", () => {
  const pool = (addr: string, resource_a: ResourceAddress, resource_b: ResourceAddress): SwapPoolInfo => ({
    pool_address: addr,
    resource_a,
    balance_a: 1_000_000n,
    resource_b,
    balance_b: 1_000_000n,
  });

  it("keeps only pools pairing the token with TARI in either order", () => {
    const pools = [
      pool("component_01", TARI_TOKEN, TOKEN_A),
      pool("component_02", TOKEN_A, TARI_TOKEN),
      pool("component_03", TOKEN_A, TOKEN_B),
      pool("component_04", TARI_TOKEN, TOKEN_B),
    ];

    expect(filterTariSwapPools(pools, TOKEN_A).map((p) => p.pool_address)).toEqual(["component_01", "component_02"]);
  });

  it("returns empty when resourceAddress is missing or is TARI itself", () => {
    const pools = [pool("component_01", TARI_TOKEN, TOKEN_A)];
    expect(filterTariSwapPools(pools, undefined)).toEqual([]);
    expect(filterTariSwapPools(pools, TARI_TOKEN)).toEqual([]);
  });
});
