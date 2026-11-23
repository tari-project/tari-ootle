//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { formatManifestCode, parseManifestFee } from "./manifestHelpers";

describe("formatManifestCode", () => {
  it("preserves outer indentation after closing a nested block", () => {
    const input = ["let a = {", "if x {", "foo();", "}", "bar();", "};"].join("\n");
    const expected = ["let a = {", "    if x {", "        foo();", "    }", "    bar();", "};"].join("\n");
    expect(formatManifestCode(input)).toBe(expected);
  });

  it("indents else blocks properly when a line both closes and opens a brace", () => {
    const input = ["if a {", "x();", "} else {", "y();", "}"].join("\n");
    const expected = ["if a {", "    x();", "} else {", "    y();", "}"].join("\n");
    expect(formatManifestCode(input)).toBe(expected);
  });

  it("handles multiple leading closing braces and same-line brace pairs", () => {
    const input = ["fn main() {", "let x = {", "call({ a: 1 });", "}}", "next();"].join("\n");
    const expected = ["fn main() {", "    let x = {", "        call({ a: 1 });", "}}", "next();"].join("\n");
    expect(formatManifestCode(input)).toBe(expected);
  });

  it("preserves blank lines and clamps extra closing braces to zero indent", () => {
    const input = ["}", "", "let a = 1;"].join("\n");
    expect(formatManifestCode(input)).toBe("}\n\nlet a = 1;");
  });
});

describe("parseManifestFee", () => {
  it("treats blank input and zero as dry-run estimate mode", () => {
    expect(parseManifestFee("")).toEqual({ kind: "estimate" });
    expect(parseManifestFee("   ")).toEqual({ kind: "estimate" });
    expect(parseManifestFee("0")).toEqual({ kind: "estimate" });
    expect(parseManifestFee("000")).toEqual({ kind: "estimate" });
  });

  it("parses positive integers as submit mode", () => {
    expect(parseManifestFee("1")).toEqual({ kind: "submit", value: 1n });
    expect(parseManifestFee(" 5000 ")).toEqual({ kind: "submit", value: 5000n });
    expect(parseManifestFee("18446744073709551615")).toEqual({ kind: "submit", value: 18446744073709551615n });
  });

  it("rejects negative, fractional, scientific-notation, and non-numeric inputs without throwing", () => {
    expect(parseManifestFee("-10")).toEqual({ kind: "invalid" });
    expect(parseManifestFee("0.5")).toEqual({ kind: "invalid" });
    expect(parseManifestFee("1e5")).toEqual({ kind: "invalid" });
    expect(parseManifestFee("abc")).toEqual({ kind: "invalid" });
  });
});
