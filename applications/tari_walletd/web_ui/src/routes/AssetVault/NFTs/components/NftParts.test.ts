// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { extractNftMetadata, formatNftDataValue, getNftImageUrl } from "./NftParts";

describe("getNftImageUrl", () => {
  it("returns a trimmed non-empty string image_url from plain mutable data", () => {
    expect(getNftImageUrl({ image_url: "  https://cdn.example/nft.png  " })).toBe("https://cdn.example/nft.png");
  });

  it("returns undefined for blank, non-string, or non-object inputs", () => {
    expect(getNftImageUrl({ image_url: "   " })).toBeUndefined();
    expect(getNftImageUrl({ image_url: 123 })).toBeUndefined();
    expect(getNftImageUrl(null)).toBeUndefined();
    expect(getNftImageUrl(["https://cdn.example/nft.png"])).toBeUndefined();
    expect(getNftImageUrl("https://cdn.example/nft.png")).toBeUndefined();
  });
});

describe("formatNftDataValue", () => {
  it("formats primitives and BigInt values without throwing", () => {
    expect(formatNftDataValue("gold")).toBe("gold");
    expect(formatNftDataValue(7)).toBe("7");
    expect(formatNftDataValue(true)).toBe("true");
    expect(formatNftDataValue(9007199254740993n)).toBe("9007199254740993");
    expect(formatNftDataValue(null)).toBe("");
    expect(formatNftDataValue(undefined)).toBe("");
  });

  it("serializes nested objects and arrays containing BigInt instead of [object Object]", () => {
    expect(formatNftDataValue({ tier: "rare", power: 9007199254740995n })).toBe(
      '{"tier":"rare","power":"9007199254740995"}',
    );
    expect(formatNftDataValue([1, 2n])).toBe('[1,"2"]');
  });
});

describe("extractNftMetadata", () => {
  it("extracts primitive and BigInt entries up to the limit and skips nested structures", () => {
    const data = {
      name: "Ootle Pass",
      edition: 1,
      verified: true,
      serial: 9007199254740993n,
      nested: { ignored: true },
      tags: ["ignored"],
      color_bg: "blue",
      color_fg: "blue",
    };

    expect(extractNftMetadata(data)).toEqual([
      ["name", "Ootle Pass"],
      ["edition", "1"],
      ["verified", "true"],
      ["serial", "9007199254740993"],
      ["color_bg", "blue"],
    ]);
  });

  it("returns an empty array for non-plain objects such as arrays, strings, or null", () => {
    expect(extractNftMetadata(null)).toEqual([]);
    expect(extractNftMetadata("plain-string")).toEqual([]);
    expect(extractNftMetadata(["a", "b"])).toEqual([]);
  });
});
