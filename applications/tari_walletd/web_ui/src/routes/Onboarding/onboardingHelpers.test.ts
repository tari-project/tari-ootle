// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { resolveOnboardingView } from "./onboardingHelpers";

describe("resolveOnboardingView", () => {
  it("redirects home when store account is already present even if an error exists", () => {
    const err = new Error("Not found", { cause: { code: 404 } });
    expect(resolveOnboardingView({ account: { name: "Primary" }, defaultAccount: undefined, error: err })).toEqual({
      kind: "redirect-home",
    });
  });

  it("redirects home when defaultAccount resolves", () => {
    expect(
      resolveOnboardingView({ account: undefined, defaultAccount: { account: { name: "Default" } }, error: null }),
    ).toEqual({ kind: "redirect-home" });
  });

  it("shows the onboarding form on a 404 default-account error", () => {
    const err = new Error("Not found", { cause: { code: 404 } });
    expect(resolveOnboardingView({ account: undefined, defaultAccount: undefined, error: err })).toEqual({
      kind: "form",
    });
  });

  it("shows an error view with fallback text on non-404 failures", () => {
    expect(
      resolveOnboardingView({
        account: undefined,
        defaultAccount: undefined,
        error: new Error("Daemon unreachable", { cause: { code: 503 } }),
      }),
    ).toEqual({
      kind: "error",
      message: "Daemon unreachable",
    });

    expect(
      resolveOnboardingView({
        account: undefined,
        defaultAccount: undefined,
        error: new Error("   "),
      }),
    ).toEqual({
      kind: "error",
      message: "Failed to load default account.",
    });
  });

  it("stays in loading state while default account check is pending", () => {
    expect(resolveOnboardingView({ account: undefined, defaultAccount: undefined, error: null })).toEqual({
      kind: "loading",
    });
  });
});
