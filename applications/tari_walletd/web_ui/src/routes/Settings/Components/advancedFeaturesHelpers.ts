// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import type { AdvancedUiFeatures } from "@tari-project/ootle-ts-bindings";

export function formatSettingsError(error: unknown, fallback: string): string {
  if (error instanceof Error) {
    const message = error.message.trim();
    return message || fallback;
  }
  if (typeof error === "string") {
    const message = error.trim();
    return message || fallback;
  }
  if (error && typeof error === "object" && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string" && message.trim()) {
      return message.trim();
    }
  }
  return fallback;
}

export async function persistAdvancedFeatureToggle(params: {
  current: AdvancedUiFeatures;
  feature: keyof AdvancedUiFeatures;
  enabled: boolean;
  apply: (features: AdvancedUiFeatures) => void;
  persist: (features: AdvancedUiFeatures) => Promise<unknown>;
}): Promise<{ ok: true; features: AdvancedUiFeatures } | { ok: false; error: string }> {
  const { current, feature, enabled, apply, persist } = params;
  const updated: AdvancedUiFeatures = { ...current, [feature]: enabled };
  apply(updated);
  try {
    await persist(updated);
    return { ok: true, features: updated };
  } catch (err) {
    apply(current);
    return {
      ok: false,
      error: formatSettingsError(err, "Failed to save advanced feature setting."),
    };
  }
}
