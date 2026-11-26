//  Copyright 2026. The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { getDefaultAccountFetchState } from "./accountFallback";

describe("getDefaultAccountFetchState", () => {
  it("suppresses default-account loading and error states when an account is already selected", () => {
    expect(getDefaultAccountFetchState({ hasAccount: true, isLoading: true, isError: false })).toEqual({
      showLoading: false,
      showError: false,
    });
    expect(getDefaultAccountFetchState({ hasAccount: true, isLoading: false, isError: true })).toEqual({
      showLoading: false,
      showError: false,
    });
  });

  it("forwards loading and error states when no account is selected yet", () => {
    expect(getDefaultAccountFetchState({ hasAccount: false, isLoading: true, isError: false })).toEqual({
      showLoading: true,
      showError: false,
    });
    expect(getDefaultAccountFetchState({ hasAccount: false, isLoading: false, isError: true })).toEqual({
      showLoading: false,
      showError: true,
    });
    expect(getDefaultAccountFetchState({ hasAccount: false, isLoading: false, isError: false })).toEqual({
      showLoading: false,
      showError: false,
    });
  });
});
