// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { isAccountNotFoundError } from "@api/helpers/errors";

export type MyAssetsView<TAccount> =
  | { kind: "ready"; account: TAccount }
  | { kind: "redirect-onboarding" }
  | { kind: "error"; message: string }
  | { kind: "loading" };

export function resolveMyAssetsView<TAccount>(params: {
  account: TAccount | null | undefined;
  error: Error | null;
}): MyAssetsView<TAccount> {
  if (params.account) {
    return { kind: "ready", account: params.account };
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
