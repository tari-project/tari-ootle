// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { isAccountNotFoundError } from "./errors";

describe("isAccountNotFoundError", () => {
  it("detects JSON-RPC 404 errors via error.cause.code", () => {
    expect(isAccountNotFoundError(new Error("Not found", { cause: { code: 404 } }))).toBe(true);
  });

  it("returns false for non-404 errors and non-objects", () => {
    expect(isAccountNotFoundError(new Error("Internal error", { cause: { code: 500 } }))).toBe(false);
    expect(isAccountNotFoundError(new Error("Network error"))).toBe(false);
    expect(isAccountNotFoundError(null)).toBe(false);
    expect(isAccountNotFoundError("404")).toBe(false);
  });
});
