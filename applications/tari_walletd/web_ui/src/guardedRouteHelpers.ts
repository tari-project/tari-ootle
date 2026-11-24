//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

export type GuardedRouteView = "error" | "loading" | "auth" | "render";

export interface GuardedRouteState {
  isLoading: boolean;
  authMethodsIsError: boolean;
  hasAuthMethod: boolean;
  hasToken: boolean | null;
  loggedIn: boolean;
}

export function resolveGuardedRouteView(state: GuardedRouteState): GuardedRouteView {
  if (state.authMethodsIsError) {
    return "error";
  }
  if (state.isLoading || !state.hasAuthMethod || state.hasToken === null) {
    return "loading";
  }
  if (!state.hasToken || !state.loggedIn) {
    return "auth";
  }
  return "render";
}
