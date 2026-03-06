import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const __dirname = dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  plugins: [react()],
  build: {
    lib: {
      entry: resolve(__dirname, "lib/index.ts"),
      name: "@tari-project/ootle-web-ui-theming",
      formats: ["es"],
    },
  },
  rollupOptions: {
    input: resolve(__dirname, "lib/index.ts"),
    external: ["react", "react-dom", "@mui/material"],
    output: {
      compact: true,
      validate: true,
      entryFileNames: "[name].js",
      generatedCode: {
        objectShorthand: true,
        constBindings: true,
      },
      globals: {
        "react": "React",
        "react-dom": "ReactDOM",
        "@mui/material": "MaterialUI",
      },
    },
  },
});
