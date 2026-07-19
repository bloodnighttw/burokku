import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react()],
  define: {
    "process.env.NODE_ENV": JSON.stringify("production"),
  },
  build: {
    lib: {
      entry: "src/main.tsx",
      formats: ["iife"],
      name: "BurokkuReactExample",
      fileName: () => "app.js",
    },
    minify: false,
  },
});
