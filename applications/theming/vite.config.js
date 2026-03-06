import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import dts from "unplugin-dts/vite";
const __dirname = dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  plugins: [dts({ tsconfigPath: resolve(__dirname, "tsconfig.json") })],
  build: {
    lib: {
      entry: resolve(__dirname, "lib/index.ts"),
      name: "@tari-project/ootle-web-ui-theming",
      fileName: "index",
      cssFileName: "theme",
      formats: ["es"],
    },
  },
  rollupOptions: {
    external: ["react", "react-dom", "@mui/material"],
    input: resolve(__dirname, "lib/index.ts"),
    output: {
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
