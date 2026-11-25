// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

export type WebauthnView =
  | { kind: "loading" }
  | { kind: "error"; message: string }
  | { kind: "login" }
  | { kind: "register" };

export function resolveWebauthnView(params: {
  isLoading: boolean;
  isError: boolean;
  errorMessage?: string | null;
  registered?: boolean;
}): WebauthnView {
  if (params.isError) {
    return {
      kind: "error",
      message: params.errorMessage?.trim() || "Failed to check passkey registration status.",
    };
  }

  if (params.isLoading || params.registered === undefined) {
    return { kind: "loading" };
  }

  return params.registered ? { kind: "login" } : { kind: "register" };
}
