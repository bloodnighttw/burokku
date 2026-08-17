import { defineConfig } from "vite";

export default defineConfig({
  build: {
    lib: {
      entry: "src/main.ts",
      formats: ["iife"],
      name: "BurokkuCounterExample",
      fileName: () => "app.js",
    },
    minify: false,
  },
});
