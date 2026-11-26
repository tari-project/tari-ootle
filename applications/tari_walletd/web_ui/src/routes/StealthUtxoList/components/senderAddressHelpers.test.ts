// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { formatAddressBookAddError, formatDestinationTag, parseSenderAddress } from "./senderAddressHelpers";

describe("senderAddressHelpers", () => {
  it("formats string and binary destination tags", () => {
    expect(formatDestinationTag(null)).toBeNull();
    expect(formatDestinationTag("")).toBeNull();
    expect(formatDestinationTag("invoice-42")).toBe("invoice-42");
    expect(formatDestinationTag(new Uint8Array())).toBeNull();
    expect(formatDestinationTag(new Uint8Array([0x0a, 0xff]))).toBe("0aff");
  });

  it("returns null fields when address cannot be decoded", () => {
    expect(parseSenderAddress("not-an-ootle-address")).toEqual({
      identityKey: null,
      destinationTagText: null,
    });
  });

  it("formats DuplicateName, nested cause, and non-object errors safely", () => {
    expect(formatAddressBookAddError({ cause: { message: "DuplicateName: Alice" } })).toBe(
      "An entry with this name already exists",
    );
    expect(formatAddressBookAddError(new Error("Network error"))).toBe("Network error");
    expect(formatAddressBookAddError(null)).toBe("Failed to add contact");
    expect(formatAddressBookAddError(undefined)).toBe("Failed to add contact");
  });
});
