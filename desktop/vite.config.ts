import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Tauri expects a fixed port and must not watch its own Rust build output.
export default defineConfig(({ mode }) => ({
  // DOM tests mount Svelte components using the browser entry point.
  ...(mode === "test" ? { resolve: { conditions: ["browser"] } } : {}),
  plugins: [svelte()],
  clearScreen: false,
  // The About page bundles the repository's CHANGELOG.md, one level up.
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**"] }, fs: { allow: [".", "../CHANGELOG.md"] } },
  build: { target: "es2022" },
}));
