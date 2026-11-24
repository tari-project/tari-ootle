// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { formatNetworkEpoch, formatRelativeTime, formatUnixTime } from "./validatorSetHelpers";

describe("validatorSetHelpers", () => {
  it("formats relative durations across seconds, minutes, hours, and days", () => {
    const now = 1_700_000_000_000;
    expect(formatRelativeTime(new Date(now - 15_000), now)).toBe("15s ago");
    expect(formatRelativeTime(new Date(now - 5 * 60_000), now)).toBe("5m ago");
    expect(formatRelativeTime(new Date(now - 3 * 3_600_000), now)).toBe("3h ago");
    expect(formatRelativeTime(new Date(now - 2 * 86_400_000), now)).toBe("2d ago");
    expect(formatRelativeTime(new Date(now + 10_000), now)).toBe("0s ago");
  });

  it("handles null, negative, and non-finite unix timestamps without returning Invalid Date", () => {
    const now = 1_700_000_060_000;
    expect(formatUnixTime(null, now)).toBe("Never");
    expect(formatUnixTime(undefined, now)).toBe("Never");
    expect(formatUnixTime(Number.NaN, now)).toBe("Invalid timestamp");
    expect(formatUnixTime(Number.POSITIVE_INFINITY, now)).toBe("Invalid timestamp");
    expect(formatUnixTime(-10, now)).toBe("Invalid timestamp");
    expect(formatUnixTime(1_700_000_000, now)).toContain("(1m ago)");
  });

  it("distinguishes loading settings from unreachable indexer when formatting network epoch", () => {
    expect(formatNetworkEpoch(42, false)).toBe("42");
    expect(formatNetworkEpoch(0, true)).toBe("0");
    expect(formatNetworkEpoch(null, true)).toBe("Loading...");
    expect(formatNetworkEpoch(null, false)).toBe("Unknown (indexer unreachable)");
  });
});
