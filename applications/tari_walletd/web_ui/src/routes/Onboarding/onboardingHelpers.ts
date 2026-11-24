// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

export type OnboardingView =
  | { kind: "redirect-home" }
  | { kind: "form" }
  | { kind: "error"; message: string }
  | { kind: "loading" };

export function isAccountNotFoundError(error: unknown): boolean {
  if (!error || typeof error !== "object") {
    return false;
  }
  const cause = (error as { cause?: unknown }).cause;
  return Boolean(cause && typeof cause === "object" && (cause as { code?: unknown }).code === 404);
}

export function resolveOnboardingView<TAccount, TDefaultAccount>(params: {
  account: TAccount | null | undefined;
  defaultAccount: TDefaultAccount | null | undefined;
  error: Error | null;
}): OnboardingView {
  if (params.account || params.defaultAccount) {
    return { kind: "redirect-home" };
  }
  if (params.error) {
    if (isAccountNotFoundError(params.error)) {
      return { kind: "form" };
    }
    return {
      kind: "error",
      message: params.error.message?.trim() || "Failed to load default account.",
    };
  }
  return { kind: "loading" };
}
