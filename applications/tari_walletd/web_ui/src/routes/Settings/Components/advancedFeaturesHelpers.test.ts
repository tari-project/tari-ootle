// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import type { AdvancedUiFeatures } from "@tari-project/ootle-ts-bindings";
import { describe, expect, it, vi } from "vitest";
import { formatSettingsError, persistAdvancedFeatureToggle } from "./advancedFeaturesHelpers";

describe("advancedFeaturesHelpers", () => {
  it("formats Error messages and falls back on empty or non-Error values", () => {
    expect(formatSettingsError(new Error("daemon offline"), "fallback")).toBe("daemon offline");
    expect(formatSettingsError(new Error("   "), "fallback")).toBe("fallback");
    expect(formatSettingsError(null, "fallback")).toBe("fallback");
  });

  it("applies optimistic feature update and keeps it on successful persist", async () => {
    const current: AdvancedUiFeatures = { enable_manifest: false, enable_validator_set: false };
    const apply = vi.fn();
    const persist = vi.fn().mockResolvedValue({});

    const result = await persistAdvancedFeatureToggle({
      current,
      feature: "enable_manifest",
      enabled: true,
      apply,
      persist,
    });

    expect(result).toEqual({
      ok: true,
      features: { enable_manifest: true, enable_validator_set: false },
    });
    expect(apply).toHaveBeenCalledTimes(1);
    expect(apply).toHaveBeenCalledWith({ enable_manifest: true, enable_validator_set: false });
    expect(persist).toHaveBeenCalledWith({ enable_manifest: true, enable_validator_set: false });
  });

  it("rolls back to previous features and returns formatted error when persist rejects", async () => {
    const current: AdvancedUiFeatures = { enable_manifest: true, enable_validator_set: false };
    const apply = vi.fn();
    const persist = vi.fn().mockRejectedValue(new Error("write failed"));

    const result = await persistAdvancedFeatureToggle({
      current,
      feature: "enable_validator_set",
      enabled: true,
      apply,
      persist,
    });

    expect(result).toEqual({ ok: false, error: "write failed" });
    expect(apply).toHaveBeenNthCalledWith(1, { enable_manifest: true, enable_validator_set: true });
    expect(apply).toHaveBeenNthCalledWith(2, current);
  });
});
