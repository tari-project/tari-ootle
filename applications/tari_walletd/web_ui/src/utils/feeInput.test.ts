//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { parseFeeInput } from "./helpers";

describe("parseFeeInput", () => {
  it("returns empty for blank strings", () => {
    expect(parseFeeInput("")).toEqual({ kind: "empty" });
    expect(parseFeeInput("   ")).toEqual({ kind: "empty" });
  });

  it("parses positive base-10 integer strings", () => {
    expect(parseFeeInput("1")).toEqual({ kind: "valid", value: 1n });
    expect(parseFeeInput(" 2500 ")).toEqual({ kind: "valid", value: 2500n });
    expect(parseFeeInput("18446744073709551615")).toEqual({ kind: "valid", value: 18446744073709551615n });
  });

  it("returns invalid without throwing for non-integer, zero, or negative inputs", () => {
    expect(parseFeeInput("0")).toEqual({ kind: "invalid" });
    expect(parseFeeInput("-100")).toEqual({ kind: "invalid" });
    expect(parseFeeInput("0.5")).toEqual({ kind: "invalid" });
    expect(parseFeeInput("1.0")).toEqual({ kind: "invalid" });
    expect(parseFeeInput("1e5")).toEqual({ kind: "invalid" });
    expect(parseFeeInput("abc")).toEqual({ kind: "invalid" });
    expect(parseFeeInput("100 µT")).toEqual({ kind: "invalid" });
  });
});
