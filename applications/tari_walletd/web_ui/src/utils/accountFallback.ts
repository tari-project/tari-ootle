//  Copyright 2026. The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

export interface DefaultAccountFetchState {
  showLoading: boolean;
  showError: boolean;
}

export function getDefaultAccountFetchState(
  hasAccount: boolean,
  isLoading: boolean,
  isError: boolean,
): DefaultAccountFetchState {
  if (hasAccount) {
    return { showLoading: false, showError: false };
  }
  return { showLoading: isLoading, showError: isError };
}
