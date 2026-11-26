//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import type { AuthGetMethodResponse } from "@tari-project/ootle-ts-bindings";

export type GuardedRouteDecision =
  | { view: "error" }
  | { view: "loading" }
  | { view: "auth"; authMethod: AuthGetMethodResponse }
  | { view: "render"; authMethod: AuthGetMethodResponse };

export interface GuardedRouteState {
  isLoading: boolean;
  authMethodsIsError: boolean;
  authMethod: AuthGetMethodResponse | undefined;
  hasToken: boolean | null;
  loggedIn: boolean;
}

export function resolveGuardedRouteView(state: GuardedRouteState): GuardedRouteDecision {
  if (state.authMethodsIsError) {
    return { view: "error" };
  }
  if (state.isLoading || !state.authMethod || state.hasToken === null) {
    return { view: "loading" };
  }
  if (!state.hasToken || !state.loggedIn) {
    return { view: "auth", authMethod: state.authMethod };
  }
  return { view: "render", authMethod: state.authMethod };
}
