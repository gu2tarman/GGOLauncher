import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri-friendly Vite config: dev server on 1420, disable HMR overlay
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: "127.0.0.1",
    watch: {
      // Tauri가 쓰는 디렉터리는 Vite가 감시하지 않게 차단 (EBUSY 방지)
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    target: "esnext",
    minify: "esbuild",
    sourcemap: false,
  },
});
