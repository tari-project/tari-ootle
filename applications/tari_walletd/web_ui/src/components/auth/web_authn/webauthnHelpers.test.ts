// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { resolveWebauthnView } from "./webauthnHelpers";

describe("resolveWebauthnView", () => {
  it("shows error instead of falling through to registration when the status query fails", () => {
    expect(
      resolveWebauthnView({
        isLoading: false,
        isError: true,
        errorMessage: "Failed to fetch",
        registered: undefined,
      }),
    ).toEqual({
      kind: "error",
      message: "Failed to fetch",
    });
  });

  it("uses a fallback message when an error has no message text", () => {
    expect(
      resolveWebauthnView({
        isLoading: false,
        isError: true,
        errorMessage: "   ",
        registered: undefined,
      }),
    ).toEqual({
      kind: "error",
      message: "Failed to check passkey registration status.",
    });
  });

  it("stays in loading while fetching or before registration status arrives", () => {
    expect(
      resolveWebauthnView({
        isLoading: true,
        isError: false,
        registered: undefined,
      }),
    ).toEqual({ kind: "loading" });

    expect(
      resolveWebauthnView({
        isLoading: false,
        isError: false,
        registered: undefined,
      }),
    ).toEqual({ kind: "loading" });
  });

  it("resolves directly to login when already registered without flashing registration", () => {
    expect(
      resolveWebauthnView({
        isLoading: false,
        isError: false,
        registered: true,
      }),
    ).toEqual({ kind: "login" });
  });

  it("resolves to register only when the daemon confirms no passkey is registered", () => {
    expect(
      resolveWebauthnView({
        isLoading: false,
        isError: false,
        registered: false,
      }),
    ).toEqual({ kind: "register" });
  });
});
