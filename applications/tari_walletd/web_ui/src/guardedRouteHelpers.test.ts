//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import type { AuthGetMethodResponse } from "@tari-project/ootle-ts-bindings";
import { describe, expect, it } from "vitest";
import { resolveGuardedRouteView } from "./guardedRouteHelpers";

const SAMPLE_AUTH_METHOD: AuthGetMethodResponse = { method: "webauthn" };

describe("resolveGuardedRouteView", () => {
  it("returns error when auth method fetch fails even if authMethod is missing or hasToken is null", () => {
    expect(
      resolveGuardedRouteView({
        isLoading: false,
        authMethodsIsError: true,
        authMethod: undefined,
        hasToken: null,
        loggedIn: false,
      }),
    ).toEqual({ view: "error" });
  });

  it("returns loading while auth method or initial token check is pending", () => {
    expect(
      resolveGuardedRouteView({
        isLoading: true,
        authMethodsIsError: false,
        authMethod: undefined,
        hasToken: null,
        loggedIn: false,
      }),
    ).toEqual({ view: "loading" });

    expect(
      resolveGuardedRouteView({
        isLoading: false,
        authMethodsIsError: false,
        authMethod: SAMPLE_AUTH_METHOD,
        hasToken: null,
        loggedIn: false,
      }),
    ).toEqual({ view: "loading" });
  });

  it("returns auth with narrowed authMethod when unauthenticated and render when authenticated", () => {
    expect(
      resolveGuardedRouteView({
        isLoading: false,
        authMethodsIsError: false,
        authMethod: SAMPLE_AUTH_METHOD,
        hasToken: false,
        loggedIn: false,
      }),
    ).toEqual({ view: "auth", authMethod: SAMPLE_AUTH_METHOD });

    expect(
      resolveGuardedRouteView({
        isLoading: false,
        authMethodsIsError: false,
        authMethod: SAMPLE_AUTH_METHOD,
        hasToken: true,
        loggedIn: true,
      }),
    ).toEqual({ view: "render", authMethod: SAMPLE_AUTH_METHOD });
  });
});
