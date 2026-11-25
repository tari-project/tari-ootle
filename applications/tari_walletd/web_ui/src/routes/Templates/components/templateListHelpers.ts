// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { decodeOotleAddressOrNull, TemplatesListAuthoredRequest } from "@tari-project/ootle-ts-bindings";

export function extractAuthorPublicKey(ootleAddress: string | null | undefined): string | null {
  if (!ootleAddress) {
    return null;
  }
  return decodeOotleAddressOrNull(ootleAddress)?.accountPublicKey ?? null;
}

export function buildAuthoredTemplatesQuery(
  authorPublicKey: string | null,
  page: number,
  pageSize: number,
): { request: TemplatesListAuthoredRequest; enabled: boolean } {
  return {
    request: {
      author_public_key: authorPublicKey,
      page,
      page_size: pageSize,
    },
    enabled: authorPublicKey !== null,
  };
}
