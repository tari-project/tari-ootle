//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { dark, light } from "@tari-project/ootle-web-ui-theming";
import { describe, expect, it, vi } from "vitest";
import { resolveThemeOptions, syncAdvancedUiFeatures } from "./layoutMainHelpers";

describe("layoutMainHelpers", () => {
  it("resolves light and dark theme options", () => {
    expect(resolveThemeOptions("light")).toBe(light);
    expect(resolveThemeOptions("dark")).toBe(dark);
    expect(resolveThemeOptions("unknown")).toBe(dark);
  });

  it("applies advanced UI features when not cancelled", async () => {
    const features = { enable_manifest: true, enable_validator_set: true };
    const setAdvancedUiFeatures = vi.fn();
    const onError = vi.fn();

    const applied = await syncAdvancedUiFeatures({
      fetchSettings: async () => ({ advanced_ui_features: features }),
      setAdvancedUiFeatures,
      isCancelled: () => false,
      onError,
    });

    expect(applied).toBe(true);
    expect(setAdvancedUiFeatures).toHaveBeenCalledWith(features);
    expect(onError).not.toHaveBeenCalled();
  });

  it("ignores resolved settings after logout or unmount", async () => {
    let cancelled = false;
    const setAdvancedUiFeatures = vi.fn();

    const applied = await syncAdvancedUiFeatures({
      fetchSettings: async () => {
        cancelled = true;
        return { advanced_ui_features: { enable_manifest: true, enable_validator_set: false } };
      },
      setAdvancedUiFeatures,
      isCancelled: () => cancelled,
    });

    expect(applied).toBe(false);
    expect(setAdvancedUiFeatures).not.toHaveBeenCalled();
  });

  it("catches settings fetch failures without throwing unhandled rejections", async () => {
    const err = new Error("daemon unreachable");
    const setAdvancedUiFeatures = vi.fn();
    const onError = vi.fn();

    const applied = await syncAdvancedUiFeatures({
      fetchSettings: async () => {
        throw err;
      },
      setAdvancedUiFeatures,
      isCancelled: () => false,
      onError,
    });

    expect(applied).toBe(false);
    expect(setAdvancedUiFeatures).not.toHaveBeenCalled();
    expect(onError).toHaveBeenCalledWith(err);
  });
});
