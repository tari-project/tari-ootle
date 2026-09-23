//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// https://vite.dev/config/
export default defineConfig({
  build: {
    // Keep .gitkeep
    emptyOutDir: false,
  },
  plugins: [react()],
});
