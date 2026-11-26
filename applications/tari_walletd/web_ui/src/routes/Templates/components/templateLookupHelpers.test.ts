// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { normalizeTemplateAddress, resolveTemplateLookupAction } from "./templateLookupHelpers";

describe("normalizeTemplateAddress", () => {
  it("trims whitespace and strips a leading template_ prefix", () => {
    expect(normalizeTemplateAddress("  template_abcd1234  ")).toBe("abcd1234");
    expect(normalizeTemplateAddress("abcd1234")).toBe("abcd1234");
  });

  it("returns an empty string for blank or prefix-only input", () => {
    expect(normalizeTemplateAddress("")).toBe("");
    expect(normalizeTemplateAddress("   ")).toBe("");
    expect(normalizeTemplateAddress("template_")).toBe("");
    expect(normalizeTemplateAddress("  template_   ")).toBe("");
  });
});

describe("resolveTemplateLookupAction", () => {
  it("ignores blank or prefix-only inputs", () => {
    expect(resolveTemplateLookupAction("", "")).toEqual({ kind: "ignore" });
    expect(resolveTemplateLookupAction("template_", "abcd")).toEqual({ kind: "ignore" });
  });

  it("returns set-address for a new normalized template address", () => {
    expect(resolveTemplateLookupAction("template_abcd", "")).toEqual({
      kind: "set-address",
      address: "abcd",
    });
    expect(resolveTemplateLookupAction("ef01", "abcd")).toEqual({
      kind: "set-address",
      address: "ef01",
    });
  });

  it("returns refetch when retrying the same template address with or without prefix", () => {
    expect(resolveTemplateLookupAction("abcd", "abcd")).toEqual({ kind: "refetch" });
    expect(resolveTemplateLookupAction("  template_abcd  ", "abcd")).toEqual({ kind: "refetch" });
  });
});
