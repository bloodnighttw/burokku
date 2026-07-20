import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

export default defineConfig({
  plugins: [
    solid({
      solid: {
        generate: "universal",
        moduleName: "@burokku/solid",
      },
    }),
  ],
  build: {
    lib: {
      entry: "src/main.tsx",
      formats: ["iife"],
      name: "BurokkuSolidExample",
      fileName: () => "app.js",
    },
    minify: false,
  },
});
