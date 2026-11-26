// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import type { Instruction } from "@tari-project/ootle-ts-bindings";
import { encode } from "cbor2";
import { describe, expect, it } from "vitest";
import { inspectify, toJsonFriendly } from "./Instructions";

const hex = (bytes: Uint8Array) => Buffer.from(bytes).toString("hex");

describe("toJsonFriendly", () => {
  it("converts BigInt, Uint8Array, Date, and nested Map values into JSON-safe structures", () => {
    const date = new Date("2026-01-02T03:04:05.000Z");
    const nested = new Map<unknown, unknown>([
      ["big", 9007199254740993n],
      ["bytes", new Uint8Array([0x0a, 0xbc])],
      ["when", date],
      [
        "inner",
        {
          list: [1n, new Uint8Array([0xff])],
          map: new Map<unknown, unknown>([[42, 7n]]),
        },
      ],
    ]);

    const result = toJsonFriendly(nested);
    expect(result).toEqual({
      big: "9007199254740993",
      bytes: "0x0abc",
      when: "2026-01-02T03:04:05.000Z",
      inner: {
        list: ["1", "0xff"],
        map: { "42": "7" },
      },
    });
    expect(() => JSON.stringify(result)).not.toThrow();
  });
});

describe("inspectify", () => {
  it("normalizes decoded CBOR literal and blob arguments", () => {
    const literalHex = hex(encode(new Map<unknown, unknown>([["amount", 9007199254740993n]])));
    const blobBase64 = Buffer.from(encode(new Uint8Array([0xde, 0xad]))).toString("base64");
    const instruction: Instruction = {
      CallMethod: {
        call: { Address: "component_01" },
        method: "transfer",
        args: [{ Literal: literalHex }, { Blob: 0 }],
      },
    };

    const inspected = inspectify(instruction, [blobBase64]);
    expect(inspected).toEqual({
      CallMethod: {
        call: { Address: "component_01" },
        method: "transfer",
        args: [{ Literal: { amount: "9007199254740993" } }, { Blob: { index: 0, value: "0xdead" } }],
      },
    });
    expect(() => JSON.stringify(inspected)).not.toThrow();
  });

  it("falls back to 0x-prefixed hex when a literal argument is not valid CBOR", () => {
    const instruction: Instruction = {
      CallFunction: {
        address: "ab".repeat(32),
        function: "demo",
        args: [{ Literal: "ff" }],
      },
    };

    expect(inspectify(instruction, undefined)).toEqual({
      CallFunction: {
        address: "ab".repeat(32),
        function: "demo",
        args: [{ Literal: "0xff" }],
      },
    });
  });
});
