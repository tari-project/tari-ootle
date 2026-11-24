// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

export type MyAssetsView =
  | { kind: "ready" }
  | { kind: "redirect-onboarding" }
  | { kind: "error"; message: string }
  | { kind: "loading" };

export function isAccountNotFoundError(error: unknown): boolean {
  if (!error || typeof error !== "object") {
    return false;
  }
  const cause = (error as { cause?: unknown }).cause;
  return Boolean(cause && typeof cause === "object" && (cause as { code?: unknown }).code === 404);
}

export function resolveMyAssetsView(params: { hasAccount: boolean; error: Error | null }): MyAssetsView {
  if (params.hasAccount) {
    return { kind: "ready" };
  }
  if (params.error) {
    if (isAccountNotFoundError(params.error)) {
      return { kind: "redirect-onboarding" };
    }
    return {
      kind: "error",
      message: params.error.message?.trim() || "Failed to load default account.",
    };
  }
  return { kind: "loading" };
}
