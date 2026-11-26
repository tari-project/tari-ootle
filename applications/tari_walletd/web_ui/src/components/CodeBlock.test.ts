// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { stringifyContent } from "./CodeBlock";

describe("stringifyContent", () => {
  it("serializes BigInt values above Number.MAX_SAFE_INTEGER as decimal strings", () => {
    const large = 9007199254740993n;
    expect(JSON.parse(stringifyContent({ amount: large, nested: [large + 1n] }))).toEqual({
      amount: "9007199254740993",
      nested: ["9007199254740994"],
    });
  });

  it("serializes nested Uint8Array values as 0x-prefixed hex strings", () => {
    expect(JSON.parse(stringifyContent({ bytes: new Uint8Array([0x01, 0xab, 0xff]) }))).toEqual({
      bytes: "0x01abff",
    });
  });

  it("serializes Maps with string and non-string keys without collisions", () => {
    const map = new Map<unknown, unknown>([
      ["label", "ok"],
      [new Uint8Array([0xde, 0xad]), 1n],
      [new Uint8Array([0xbe, 0xef]), 2n],
      [{ id: 1 }, "first"],
      [{ id: 2 }, "second"],
    ]);
    expect(JSON.parse(stringifyContent(map))).toEqual({
      "label": "ok",
      "0xdead": "1",
      "0xbeef": "2",
      '{"id":1}': "first",
      '{"id":2}': "second",
    });
  });

  it('returns "null" for undefined content', () => {
    expect(stringifyContent(undefined)).toBe("null");
  });
});
