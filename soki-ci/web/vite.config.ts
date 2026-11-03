import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

const target = process.env.SOKI_CI_DEV_API ?? "http://127.0.0.1:9847";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    port: 5173,
    proxy: {
      "/api": { target, changeOrigin: false },
      "/health": { target, changeOrigin: false },
      "/capabilities": { target, changeOrigin: false },
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    sourcemap: false,
  },
});
