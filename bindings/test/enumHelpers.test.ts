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
    type KeyIdLike =
      | { Derived: { key_branch: string; index: number | bigint | string } }
      | { Imported: { local_key_id: number | bigint | string } };
    const daemonKeyId: KeyIdLike = { Derived: { key_branch: "account", index: 0 } };
    const reordered: KeyIdLike = { Derived: { index: 0, key_branch: "account" } };
    const claimFeesKeyId: KeyIdLike = { Derived: { key_branch: "account", index: 0n } };
    const differentIndex: KeyIdLike = { Derived: { key_branch: "account", index: 1 } };

    expect(matchesTypeEnum(daemonKeyId, reordered)).toBe(true);
    expect(matchesTypeEnum(daemonKeyId, claimFeesKeyId)).toBe(true);
    expect(matchesTypeEnum(claimFeesKeyId, daemonKeyId)).toBe(true);
    expect(matchesTypeEnum(daemonKeyId, differentIndex)).toBe(false);
    expect(matchesTypeEnum(claimFeesKeyId, differentIndex)).toBe(false);
    expect(matchesTypeEnum({ Derived: { key_branch: "account", index: 0.5 } }, claimFeesKeyId)).toBe(false);
    expect(matchesTypeEnum({ Derived: { key_branch: "account", index: "0" } }, daemonKeyId)).toBe(false);
  });

  it("compares typed arrays and DataViews by type and contents", () => {
    type BytesEnum = { Bytes: ArrayBufferView };
    expect(matchesTypeEnum<BytesEnum>({ Bytes: new Uint8Array([1, 2, 3]) }, { Bytes: new Uint8Array([1, 2, 3]) })).toBe(
      true,
    );
    expect(matchesTypeEnum<BytesEnum>({ Bytes: new Uint16Array([1, 512]) }, { Bytes: new Uint16Array([1, 512]) })).toBe(
      true,
    );
    expect(matchesTypeEnum<BytesEnum>({ Bytes: new Uint8Array([1, 0]) }, { Bytes: new Uint16Array([1]) })).toBe(false);
    const dvA = new DataView(new Uint8Array([10, 20]).buffer);
    const dvB = new DataView(new Uint8Array([10, 20]).buffer);
    const dvC = new DataView(new Uint8Array([10, 21]).buffer);
    expect(matchesTypeEnum<BytesEnum>({ Bytes: dvA }, { Bytes: dvB })).toBe(true);
    expect(matchesTypeEnum<BytesEnum>({ Bytes: dvA }, { Bytes: dvC })).toBe(false);
  });

  it("throws a descriptive error for non-object or array enum inputs", () => {
    expect(() => matchesTypeEnum(123 as unknown as CustomEnum, { A: "test" })).toThrow(
      "Enum object must be a string unit variant or a single-key object",
    );
    expect(() => matchesTypeEnum(["A"] as unknown as CustomEnum, { A: "test" })).toThrow(
      "Enum object must be a string unit variant or a single-key object",
    );
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
