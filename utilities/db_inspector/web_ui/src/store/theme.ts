//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause


import { persist } from "zustand/middleware";
import { createWithEqualityFn } from "zustand/traditional";

export type ThemeMode = "light" | "dark" | "auto";

interface Store {
  themeMode: ThemeMode,
  setThemeMode: (mode: ThemeMode) => void;
}

const useThemeStore = createWithEqualityFn<Store>()(
  persist<Store>(
    (set) => ({
      themeMode: "auto",
      setThemeMode: (mode: ThemeMode) => set({ themeMode: mode }),
    }),
    {
      name: "theme",
    },
  ),
);

export default useThemeStore;
