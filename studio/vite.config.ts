import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Tauri serves the built files itself; the dev server is only for `tauri dev`.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  // The release notes (../releases) are bundled into Settings → About.
  server: { port: 5173, strictPort: true, fs: { allow: [".", "../releases"] } },
  build: { target: "chrome120", outDir: "dist", emptyOutDir: true },
});
