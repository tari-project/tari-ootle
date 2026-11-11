//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { bigintToDecimalString, divisibilityOrDefault } from "./helpers";

describe("divisibilityOrDefault", () => {
  it("keeps a divisibility of zero for whole-token resources", () => {
    expect(divisibilityOrDefault(0, 6)).toBe(0);
  });

  it("uses the fallback only when divisibility is missing", () => {
    expect(divisibilityOrDefault(undefined, 6)).toBe(6);
    expect(divisibilityOrDefault(null, 6)).toBe(6);
    expect(divisibilityOrDefault(6, 18)).toBe(6);
  });

  it("formats one base unit of a zero-divisibility token as 1", () => {
    const decimals = divisibilityOrDefault(0, 6);
    expect(bigintToDecimalString(1n, decimals)).toBe("1");
  });
});
