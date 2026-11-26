//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import type { Permission } from "@tari-project/ootle-ts-bindings";

export interface AuthNoneClient {
  authRequest: (permissions: Permission[], method: "None") => Promise<string>;
  setToken: (token: string) => void;
}

export interface RunAuthNoneOptions {
  getClient: () => Promise<AuthNoneClient>;
  permissions: Permission[];
  isCancelled: () => boolean;
  onAuthenticated: () => void;
  onError: (error: Error) => void;
}

export function normalizeAuthError(err: unknown): Error {
  return err instanceof Error ? err : new Error(String(err ?? "Unknown authentication error"));
}

export async function runAuthNone({
  getClient,
  permissions,
  isCancelled,
  onAuthenticated,
  onError,
}: RunAuthNoneOptions): Promise<void> {
  try {
    const client = await getClient();
    const token = await client.authRequest(permissions, "None");
    if (isCancelled()) {
      return;
    }
    client.setToken(token);
    onAuthenticated();
  } catch (err) {
    if (isCancelled()) {
      return;
    }
    onError(normalizeAuthError(err));
  }
}
