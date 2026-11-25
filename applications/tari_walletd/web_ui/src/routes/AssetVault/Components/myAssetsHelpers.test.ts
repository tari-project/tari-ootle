// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { isAccountNotFoundError, resolveMyAssetsView } from "./myAssetsHelpers";

describe("isAccountNotFoundError", () => {
  it("detects JSON-RPC 404 errors via error.cause.code", () => {
    const err = new Error("Not found", { cause: { code: 404 } });
    expect(isAccountNotFoundError(err)).toBe(true);
  });

  it("returns false for non-404 errors and non-objects", () => {
    expect(isAccountNotFoundError(new Error("Internal error", { cause: { code: 500 } }))).toBe(false);
    expect(isAccountNotFoundError(new Error("Network error"))).toBe(false);
    expect(isAccountNotFoundError(null)).toBe(false);
    expect(isAccountNotFoundError("404")).toBe(false);
  });
});

describe("resolveMyAssetsView", () => {
  it("returns ready with the narrowed account when an account is loaded", () => {
    const account = { name: "Default" };
    const stale404 = new Error("Default account not found", { cause: { code: 404 } });
    expect(resolveMyAssetsView({ account, error: stale404 })).toEqual({ kind: "ready", account });
  });

  it("redirects to onboarding when no account is loaded and default-account lookup returns 404", () => {
    const notFound = new Error("Default account not found", { cause: { code: 404 } });
    expect(resolveMyAssetsView({ account: null, error: notFound })).toEqual({ kind: "redirect-onboarding" });
  });

  it("shows an error instead of infinite loading for non-404 errors when no account is loaded", () => {
    const err = new Error("Daemon unreachable", { cause: { code: 503 } });
    expect(resolveMyAssetsView({ account: null, error: err })).toEqual({
      kind: "error",
      message: "Daemon unreachable",
    });

    const emptyErr = new Error("   ");
    expect(resolveMyAssetsView({ account: null, error: emptyErr })).toEqual({
      kind: "error",
      message: "Failed to load default account.",
    });
  });

  it("shows loading when no account is loaded and no error has occurred", () => {
    expect(resolveMyAssetsView({ account: null, error: null })).toEqual({ kind: "loading" });
  });
});
