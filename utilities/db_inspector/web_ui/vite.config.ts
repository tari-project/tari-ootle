//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// https://vite.dev/config/
export default defineConfig({
  build: {
    // Keep .gitkeep
    emptyOutDir: false,
  },
  plugins: [react()],
});
