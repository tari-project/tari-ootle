// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { encodeOotleAddress } from "@tari-project/ootle-ts-bindings";
import { describe, expect, it } from "vitest";
import { buildAuthoredTemplatesQuery, extractAuthorPublicKey } from "./templateListHelpers";

describe("extractAuthorPublicKey", () => {
  it("returns null for missing or malformed addresses without throwing", () => {
    expect(extractAuthorPublicKey(undefined)).toBeNull();
    expect(extractAuthorPublicKey("")).toBeNull();
    expect(extractAuthorPublicKey("not-a-valid-ootle-address")).toBeNull();
  });

  it("extracts the account public key from a valid ootle address", () => {
    const accountPublicKey = "11".repeat(32);
    const viewOnlyKey = "22".repeat(32);
    const encoded = encodeOotleAddress({
      network: "localnet",
      accountPublicKey,
      viewOnlyKey,
      destinationTag: null,
    });
    expect(extractAuthorPublicKey(encoded)).toBe(accountPublicKey);
  });
});

describe("buildAuthoredTemplatesQuery", () => {
  it("disables the query and uses null (never empty string) when no author key is available", () => {
    expect(buildAuthoredTemplatesQuery(null, 0, 10)).toEqual({
      request: {
        author_public_key: null,
        page: 0,
        page_size: 10,
      },
      enabled: false,
    });
  });

  it("enables the query when an author public key is present", () => {
    const key = "aa".repeat(32);
    expect(buildAuthoredTemplatesQuery(key, 2, 25)).toEqual({
      request: {
        author_public_key: key,
        page: 2,
        page_size: 25,
      },
      enabled: true,
    });
  });
});
