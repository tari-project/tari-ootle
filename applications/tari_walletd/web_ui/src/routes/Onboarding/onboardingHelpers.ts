// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { isAccountNotFoundError } from "@api/helpers/errors";

export type OnboardingView =
  | { kind: "redirect-home" }
  | { kind: "form" }
  | { kind: "error"; message: string }
  | { kind: "loading" };

export function resolveOnboardingView(params: {
  account: unknown;
  defaultAccount: unknown;
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
