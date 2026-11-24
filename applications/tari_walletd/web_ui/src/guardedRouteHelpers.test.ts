//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { resolveGuardedRouteView } from "./guardedRouteHelpers";

describe("resolveGuardedRouteView", () => {
  it("returns error when auth method fetch fails even if authMethod is missing or hasToken is null", () => {
    expect(
      resolveGuardedRouteView({
        isLoading: false,
        authMethodsIsError: true,
        hasAuthMethod: false,
        hasToken: null,
        loggedIn: false,
      }),
    ).toBe("error");
  });

  it("returns loading while auth method or initial token check is pending", () => {
    expect(
      resolveGuardedRouteView({
        isLoading: true,
        authMethodsIsError: false,
        hasAuthMethod: false,
        hasToken: null,
        loggedIn: false,
      }),
    ).toBe("loading");

    expect(
      resolveGuardedRouteView({
        isLoading: false,
        authMethodsIsError: false,
        hasAuthMethod: true,
        hasToken: null,
        loggedIn: false,
      }),
    ).toBe("loading");
  });

  it("returns auth when unauthenticated and render when authenticated", () => {
    expect(
      resolveGuardedRouteView({
        isLoading: false,
        authMethodsIsError: false,
        hasAuthMethod: true,
        hasToken: false,
        loggedIn: false,
      }),
    ).toBe("auth");

    expect(
      resolveGuardedRouteView({
        isLoading: false,
        authMethodsIsError: false,
        hasAuthMethod: true,
        hasToken: true,
        loggedIn: true,
      }),
    ).toBe("render");
  });
});
