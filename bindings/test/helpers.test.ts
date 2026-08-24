//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { shortenString, shortenSubstateId } from "../src";

// The ellipsis inserted by shortenString is 3 characters, so a value is only
// worth shortening once it is at least `start + end + 3` characters long.
const DEFAULT_THRESHOLD = 8 + 8 + 3;

describe("shortenString", () => {
  // Regression for #1872: NFT metadata values such as `name: Rick Astley` were rendered as
  // `Rick Ast...k Astley`, because the head and the tail slice overlap for short values.
  it("leaves a short plain-text value untouched", () => {
    expect(shortenString("Rick Astley")).toBe("Rick Astley");
  });

  it("leaves a very short plain-text value untouched", () => {
    expect(shortenString("Hi")).toBe("Hi");
  });

  it("leaves an empty string empty rather than rendering a bare ellipsis", () => {
    expect(shortenString("")).toBe("");
  });

  it("leaves a value one character below the threshold untouched", () => {
    const value = "a".repeat(DEFAULT_THRESHOLD - 1);
    expect(value).toHaveLength(18);
    expect(shortenString(value)).toBe(value);
  });

  it("shortens a value at the threshold", () => {
    const value = "abcdefghijklmnopqrs";
    expect(value).toHaveLength(DEFAULT_THRESHOLD);
    expect(shortenString(value)).toBe("abcdefgh...lmnopqrs");
  });

  it("shortens a long plain-text value head-and-tail", () => {
    const value = "Never gonna give you up, never gonna let you down";
    expect(shortenString(value)).toBe("Never go...you down");
  });

  it("honours custom start and end lengths", () => {
    expect(shortenString("abcdefghijklmnopqrstuvwxyz", 4, 4)).toBe("abcd...wxyz");
    // The threshold follows start/end: 4 + 4 + 3 = 11, so 10 characters are left alone
    // and 11 characters are already worth shortening.
    expect(shortenString("abcdefghij", 4, 4)).toBe("abcdefghij");
    expect(shortenString("abcdefghijk", 4, 4)).toBe("abcd...hijk");
  });
});

describe("shortenSubstateId", () => {
  // These are the address forms NFT metadata values are checked against before any
  // plain-text shortening happens; #1872 must not change how they render.
  const VAULT_ID = "vault_0f987d031de55aee41a7233426059b1c3506408832f3283eb2bdaed15a314021";
  const RESOURCE_ID = "resource_0101010101010101010101010101010101010101010101010101010101010101";
  const COMPONENT_ID = "component_23e5679a3e55e58e32318b94e258b73e72e3164b658f187fe5de833a861e2d45";

  it("keeps the prefix and shortens only the payload of a vault id", () => {
    expect(shortenSubstateId(VAULT_ID)).toBe("vault_0f98...4021");
  });

  it("keeps the prefix and shortens only the payload of a resource id", () => {
    expect(shortenSubstateId(RESOURCE_ID)).toBe("resource_0101...0101");
  });

  it("keeps the prefix and shortens only the payload of a component id", () => {
    expect(shortenSubstateId(COMPONENT_ID)).toBe("component_23e5...2d45");
  });

  it("returns an empty string for null and undefined", () => {
    expect(shortenSubstateId(null)).toBe("");
    expect(shortenSubstateId(undefined)).toBe("");
  });

  it("returns a value without a prefix separator unchanged", () => {
    expect(shortenSubstateId("no-underscore-here")).toBe("no-underscore-here");
  });
});
