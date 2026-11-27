//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import type { AdvancedUiFeatures } from "@tari-project/ootle-ts-bindings";
import { dark, light } from "@tari-project/ootle-web-ui-theming";

export function resolveThemeOptions(mode: string) {
  return mode === "light" ? light : dark;
}

export async function syncAdvancedUiFeatures({
  fetchSettings,
  setAdvancedUiFeatures,
  isCancelled,
  onError = (error: unknown) => console.error("Failed to load wallet settings:", error),
}: {
  fetchSettings: () => Promise<{ advanced_ui_features: AdvancedUiFeatures }>;
  setAdvancedUiFeatures: (features: AdvancedUiFeatures) => void;
  isCancelled: () => boolean;
  onError?: (error: unknown) => void;
}): Promise<boolean> {
  try {
    const res = await fetchSettings();
    if (isCancelled()) {
      return false;
    }
    setAdvancedUiFeatures(res.advanced_ui_features);
    return true;
  } catch (error) {
    if (!isCancelled()) {
      onError(error);
    }
    return false;
  }
}
