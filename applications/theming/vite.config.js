import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  build: {
    lib: {
      name: "TariOotleTheming",
      entry: ["lib/index.ts"],
      formats: ["es"],
      fileName: "tari-ootle-theming",
      cssFileName: "theme",
    },
  },
  rollupOptions: {
    external: ["react", "react-dom", "@mui/material"],
    output: {
      globals: {
        "react": "React",
        "react-dom": "ReactDOM",
        "@mui/material": "MaterialUI",
      },
    },
  },
});
