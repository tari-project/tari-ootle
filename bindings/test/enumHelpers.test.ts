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

  it("throws an error if enum object has no keys", () => {
    const enumObject = {} as unknown as CustomEnum;
    const value = { A: "test" };
    expect(() => matchesTypeEnum(enumObject, value)).toThrow("Enum object must have exactly one key");
  });

  it("handles null inputs", () => {
    expect(matchesTypeEnum(null, null)).toBe(true);
    expect(matchesTypeEnum(null, { B: 456 })).toBe(false);
    expect(matchesTypeEnum({ B: 456 }, null)).toBe(false);
  });

  it("matches nested struct enum payloads structurally", () => {
    type KeyIdLike = { Derived: { key_branch: string; index: number } } | { Imported: { local_key_id: number } };
    const a: KeyIdLike = { Derived: { key_branch: "account", index: 0 } };
    const b: KeyIdLike = { Derived: { index: 0, key_branch: "account" } };
    const c: KeyIdLike = { Derived: { key_branch: "account", index: 1 } };

    expect(matchesTypeEnum(a, b)).toBe(true);
    expect(matchesTypeEnum(a, c)).toBe(false);
    expect(matchesTypeEnum(a, { Derived: { key_branch: "account", index: 0n } } as unknown as KeyIdLike)).toBe(false);
  });

  it("matches tuple and array enum payloads structurally", () => {
    type PermissionLike = { Accounts: [string, string | null] } | { U256: number[] };
    expect(matchesTypeEnum<PermissionLike>({ Accounts: ["Read", null] }, { Accounts: ["Read", null] })).toBe(true);
    expect(matchesTypeEnum<PermissionLike>({ Accounts: ["Read", null] }, { Accounts: ["Read", "component_00"] })).toBe(
      false,
    );
    expect(matchesTypeEnum<PermissionLike>({ U256: [1, 2, 3] }, { U256: [1, 2, 3] })).toBe(true);
    expect(matchesTypeEnum<PermissionLike>({ U256: [1, 2, 3] }, { U256: [1, 2] })).toBe(false);
  });

  it("handles string unit variants without throwing", () => {
    type MixedEnum = "None" | "Admin" | { Resource: string };
    expect(matchesTypeEnum<MixedEnum>("None", "None")).toBe(true);
    expect(matchesTypeEnum<MixedEnum>("None", "Admin")).toBe(false);
    expect(matchesTypeEnum<MixedEnum>("None", { Resource: "resource_00" })).toBe(false);
    expect(matchesTypeEnum<MixedEnum>({ Resource: "resource_00" }, "None")).toBe(false);
  });

  it("returns false when value has multiple keys", () => {
    const enumObject: CustomEnum = { A: "test" };
    const value = { A: "test", B: 123 } as unknown as CustomEnum;
    expect(matchesTypeEnum(enumObject, value)).toBe(false);
  });
});
