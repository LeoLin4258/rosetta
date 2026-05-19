import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vitejs.dev/config/
export default defineConfig(async () => ({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": "/src",
    },
  },

  // pdfjs-dist ships its worker as an .mjs that Vite tries to pre-bundle on dev
  // start, which breaks the standalone-worker loading react-pdf does. Excluding
  // it from optimizeDeps + serving worker/cmaps from /public/pdfjs (staged by
  // scripts/copy-pdfjs-assets.mjs) is the recipe react-pdf docs recommend for
  // Vite. See also: scripts/copy-pdfjs-assets.mjs
  optimizeDeps: {
    exclude: ["pdfjs-dist"],
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
