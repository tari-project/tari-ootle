// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { create } from "zustand";

interface Store {
  username: string;
  authToken: string;
  setAuthToken: (token: string) => void;
  clearToken: () => void;
}

const useAuthStore = create<Store>()(
  (set) => ({
    username: "tari-wallet-webui",
    authToken: "",
    setAuthToken: (token) => set({ authToken: token }),
    clearToken: () => set({ authToken: "" }),
  }),
);

export default useAuthStore;
