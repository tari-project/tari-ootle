//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { formatTimestamp, isSubstateIdString, parseTimestamp, shortenString, shortenSubstateId } from "./helpers";

describe("parseTimestamp", () => {
  it("parses ISO-8601 UTC timestamps ending with Z", () => {
    expect(parseTimestamp("2026-11-23T15:00:00Z")?.toISOString()).toBe("2026-11-23T15:00:00.000Z");
    expect(parseTimestamp("2026-11-23T15:00:00.123Z")?.toISOString()).toBe("2026-11-23T15:00:00.123Z");
  });

  it("parses naive timestamps as UTC", () => {
    expect(parseTimestamp("2026-11-23 15:00:00")?.toISOString()).toBe("2026-11-23T15:00:00.000Z");
    expect(parseTimestamp("2026-11-23 15:00:00.0")?.toISOString()).toBe("2026-11-23T15:00:00.000Z");
    expect(parseTimestamp("2026-11-23T15:00:00")?.toISOString()).toBe("2026-11-23T15:00:00.000Z");
  });

  it("preserves explicit numeric timezone offsets", () => {
    expect(parseTimestamp("2026-11-23T17:00:00+02:00")?.toISOString()).toBe("2026-11-23T15:00:00.000Z");
    expect(parseTimestamp("2026-11-23T10:00:00-0500")?.toISOString()).toBe("2026-11-23T15:00:00.000Z");
  });

  it("returns null for empty, nullish, or invalid timestamps", () => {
    expect(parseTimestamp(null)).toBeNull();
    expect(parseTimestamp(undefined)).toBeNull();
    expect(parseTimestamp("")).toBeNull();
    expect(parseTimestamp("   ")).toBeNull();
    expect(parseTimestamp("not-a-timestamp")).toBeNull();
  });
});

describe("formatTimestamp", () => {
  it("formats UTC Z-suffixed timestamps instead of returning an empty string", () => {
    const formatted = formatTimestamp("2026-11-23T15:00:00Z");
    expect(formatted).not.toBe("");
    expect(formatted).toContain("2026");
  });

  it("returns an empty string for nullish or invalid timestamps", () => {
    expect(formatTimestamp(null)).toBe("");
    expect(formatTimestamp(undefined)).toBe("");
    expect(formatTimestamp("invalid")).toBe("");
  });
});

describe("walletd web_ui substate helpers", () => {
  it("preserves the trailing identifier of multi-segment substate ids", () => {
    const utxoId =
      "utxo_0000000000000000000000000000000000000000000000000000000000000000_abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123459876";
    const coutputId =
      "coutput_7cbfe29101c24924b1b6ccefbfff98986d648622272ae24f7585dab5eeeeeeee_11223344556677889900aabbccddeeff11223344556677889900aabbccddff00";
    const nftId = "nft_0101010101010101010101010101010101010101010101010101010101010101_str_token_42";

    expect(shortenSubstateId(utxoId)).toBe("utxo_0000...9876");
    expect(shortenSubstateId(coutputId)).toBe("coutput_7cbf...ff00");
    expect(shortenSubstateId(nftId)).toBe("nft_0101...n_42");
  });

  it("handles null, undefined, and strings without an underscore", () => {
    expect(shortenSubstateId(null)).toBe("");
    expect(shortenSubstateId(undefined)).toBe("");
    expect(shortenSubstateId("no-underscore-here")).toBe("no-underscore-here");
    expect(shortenString(null)).toBe("");
    expect(shortenString("short")).toBe("short");
  });

  it("recognises all substate address prefixes in isSubstateIdString", () => {
    for (const prefix of [
      "component_",
      "vault_",
      "resource_",
      "nft_",
      "tombstone_",
      "txreceipt_",
      "template_",
      "vnfp_",
      "utxo_",
      "coutput_",
    ]) {
      expect(isSubstateIdString(`${prefix}0000`)).toBe(true);
    }
    expect(isSubstateIdString("account_plain_text")).toBe(false);
    expect(isSubstateIdString(null)).toBe(false);
  });
});
