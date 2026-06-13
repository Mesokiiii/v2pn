import { defineConfig } from "vite";
import solidPlugin from "vite-plugin-solid";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";

// https://vitejs.dev/config/
export default defineConfig(() => ({
  plugins: [solidPlugin(), tailwindcss()],

  resolve: {
    alias: {
      "~": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },

  // Vite options tailored for Tauri development
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    host: "127.0.0.1",
    hmr: { protocol: "ws", host: "127.0.0.1", port: 5174 },
    watch: { ignored: ["**/src-tauri/**", "**/crates/**", "**/target/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: {
    target: ["es2022"],
    minify: "esbuild" as const,
    sourcemap: false,
  },
}));
