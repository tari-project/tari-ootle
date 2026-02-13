// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { create } from "zustand";
import { setClientAuthToken } from "@utils/json_rpc";

export type AuthToken = string;

interface Store {
  username: string;
  authToken?: AuthToken;
  setAuthToken: (token: AuthToken) => void;
  clearToken: () => void;
}

const useAuthStore = create<Store>()((set) => ({
  username: "tari-wallet-webui",
  authToken: undefined,
  setAuthToken: (token) => {
    let _promise = setClientAuthToken(token);
    return set({ authToken: token });
  },
  clearToken: () =>
    set((_) => {
      return {};
    }),
}));

export default useAuthStore;
