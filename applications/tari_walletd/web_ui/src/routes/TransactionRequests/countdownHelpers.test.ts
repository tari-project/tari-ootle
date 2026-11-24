//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { formatCountdownLabel, getRemainingSeconds } from "./countdownHelpers";

describe("countdownHelpers", () => {
  it("computes non-negative remaining seconds until expiry", () => {
    expect(getRemainingSeconds(1_700_000_125n, 1_700_000_000.2)).toBe(124);
    expect(getRemainingSeconds(1_700_000_000n, 1_700_000_005.0)).toBe(0);
  });

  it("formats positive durations as m:ss and zero as expired", () => {
    expect(formatCountdownLabel(125)).toBe("expires in 2:05");
    expect(formatCountdownLabel(9)).toBe("expires in 0:09");
    expect(formatCountdownLabel(0)).toBe("expired");
  });
});
