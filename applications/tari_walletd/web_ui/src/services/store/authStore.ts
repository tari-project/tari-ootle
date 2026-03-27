// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { create } from "zustand";

interface Store {
  username: string;
  loggedIn: boolean;
  setLoggedIn: (loggedIn: boolean) => void;
  needsReauth: boolean;
  setNeedsReauth: (needsReauth: boolean) => void;
}

const useAuthStore = create<Store>()((set) => ({
  username: "tari-wallet-webui",
  loggedIn: false,
  setLoggedIn: (isLoggedIn) => set({ loggedIn: isLoggedIn }),
  needsReauth: false,
  setNeedsReauth: (needsReauth) => set({ needsReauth }),
}));

export default useAuthStore;
