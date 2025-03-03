// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { create } from "zustand";
import { persist } from "zustand/middleware";
import { AUTH_TOKEN_FOR_NONE_AUTH } from "../routes/Auth/Auth";

interface Store {
  username: string;
  authToken: string;
  setAuthToken: (token: string) => void;
  clearToken: () => void;
}

const useAuthStore = create<Store>()(
  persist<Store>(
    (set) => ({
      username: "tari-wallet-webui",
      authToken: "",
      setAuthToken: (token) => set({ authToken: token }),
      clearToken: () =>
        set((s) => {
          if (s.authToken === AUTH_TOKEN_FOR_NONE_AUTH) {
            return {};
          }
          return { authToken: "" };
        }),
    }),
    { name: "tari-auth" },
  ),
);

export default useAuthStore;
