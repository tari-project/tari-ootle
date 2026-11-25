//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
import type { Type } from "@tari-project/ootle-ts-bindings";
import { describe, expect, it } from "vitest";
import { formatType } from "./formatType";

describe("formatType", () => {
  it("formats primitive string types verbatim", () => {
    const primitives: Type[] = ["Unit", "Bool", "I8", "I64", "U8", "U64", "U128", "String"];
    for (const primitive of primitives) {
      expect(formatType(primitive)).toBe(primitive);
    }
  });

  it("wraps Vec element types in Vec<...>", () => {
    expect(formatType({ Vec: "U8" })).toBe("Vec<U8>");
    expect(formatType({ Vec: { Other: { name: "Bucket" } } })).toBe("Vec<Bucket>");
  });

  it("wraps Option inner types in Option<...>", () => {
    expect(formatType({ Option: "String" })).toBe("Option<String>");
    expect(formatType({ Option: { WellKnown: "ComponentAddress" } })).toBe("Option<ComponentAddress>");
  });

  it("formats Tuple elements recursively", () => {
    expect(formatType({ Tuple: [] })).toBe("()");
    expect(formatType({ Tuple: ["U64", { Other: { name: "Bucket" } }] })).toBe("(U64, Bucket)");
  });

  it("formats Other and WellKnown types", () => {
    expect(formatType({ Other: { name: "CustomStruct" } })).toBe("CustomStruct");
    expect(formatType({ WellKnown: "ResourceAddress" })).toBe("ResourceAddress");
  });

  it("formats deeply nested composite types", () => {
    const nested: Type = {
      Option: {
        Vec: {
          Tuple: ["U64", { Option: { Other: { name: "Bucket" } } }],
        },
      },
    };
    expect(formatType(nested)).toBe("Option<Vec<(U64, Option<Bucket>)>>");
  });
});
