// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

export const WEBAUTHN_PASSKEYS_QUERY_KEY = ["webauthn_passkeys"] as const;

export interface QueryInvalidator {
  invalidateQueries: (filters: { queryKey: readonly unknown[] }) => Promise<void>;
}

export function invalidatePasskeysList(client: QueryInvalidator): Promise<void> {
  return client.invalidateQueries({ queryKey: WEBAUTHN_PASSKEYS_QUERY_KEY });
}
