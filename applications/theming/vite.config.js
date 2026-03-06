import { resolve } from "node:path";
import { defineConfig } from "vite";
import dts from "unplugin-dts/vite";

export default defineConfig({
  build: {
    lib: {
      entry: resolve(__dirname, "lib/index.ts"),
      name: "@tari-project/ootle-web-ui-theming",
      cssFileName: "theme",
      fileName: "index",
      formats: ["es"],
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
  },
  plugins: [
    dts({
      outDirs: "dist",
      entryRoot: "lib",
      staticImport: true,
      copyDtsFiles: true,
      rollupTypes: true,
    }),
  ],
});
