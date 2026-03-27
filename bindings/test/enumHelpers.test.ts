//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { matchesTypeEnum } from "../src";

describe("matchesEnum", () => {
  type CustomEnum = { A: string } | { B: number } | { C: boolean };

  it("matches a custom enum", () => {
    const enumObject: CustomEnum | null = { A: "test" };
    const value = { A: "test" };
    expect(matchesTypeEnum(enumObject, value)).toBe(true);
  });

  it("does not match a custom enum with different key", () => {
    const enumObject: CustomEnum | null = { A: "test" };
    const value = { B: 123 } as CustomEnum;
    expect(matchesTypeEnum(enumObject, value)).toBe(false);
  });

  it("does not match a custom enum with different value type", () => {
    const enumObject: CustomEnum | null = { A: "test" };
    const value = { A: 123 } as unknown as CustomEnum;
    expect(matchesTypeEnum(enumObject, value)).toBe(false);
  });

  it("throws an error if enum object has multiple keys", () => {
    const enumObject = { A: "test", B: 123 } as unknown as CustomEnum;
    const value = { A: "test" };
    expect(() => matchesTypeEnum(enumObject, value)).toThrow("Enum object must have exactly one key");
  });

  it("returns false if enum object has no keys", () => {
    const enumObject = {} as unknown as CustomEnum;
    const value = { A: "test" };
    expect(() => matchesTypeEnum(enumObject, value)).toThrow("Enum object must have exactly one key");
  });

  it("matches enum with primitive number value", () => {
    const enumObject = null;
    const value = { B: 456 };
    expect(matchesTypeEnum(enumObject, value)).toBe(false);
    const enumObject2 = { B: 456 };
    const value2 = null;
    expect(matchesTypeEnum(enumObject2, value2)).toBe(false);
  });
});
