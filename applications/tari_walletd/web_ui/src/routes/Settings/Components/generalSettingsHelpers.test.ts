// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { formatSettingsError, getNetworkColor, shouldApplySettingsResult } from "./generalSettingsHelpers";

describe("shouldApplySettingsResult", () => {
  it("applies only when mounted and matching the latest request id", () => {
    expect(shouldApplySettingsResult(2, 2, true)).toBe(true);
    expect(shouldApplySettingsResult(1, 2, true)).toBe(false);
    expect(shouldApplySettingsResult(2, 2, false)).toBe(false);
  });
});

describe("formatSettingsError", () => {
  it("extracts Error messages and falls back cleanly for empty or unknown values", () => {
    expect(formatSettingsError(new Error("Connection refused"))).toBe("Connection refused");
    expect(formatSettingsError("RPC timeout")).toBe("RPC timeout");
    expect(formatSettingsError(new Error("   "))).toBe("Failed to load settings.");
    expect(formatSettingsError(null)).toBe("Failed to load settings.");
  });
});

describe("getNetworkColor", () => {
  it("resolves known networks case-insensitively and falls back to neutral gray", () => {
    expect(getNetworkColor("mainnet")).toBe("#4caf50");
    expect(getNetworkColor("LocalNet")).toBe("#ff9800");
    expect(getNetworkColor("unknown")).toBe("#757575");
  });
});
