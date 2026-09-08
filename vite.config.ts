import { defineConfig } from "vite";

export default defineConfig({
  server: {
    watch: {
      // macOS must not fall back to stat polling when optional fsevents is absent.
      usePolling: false,
      // Cargo owns Rust reloads; generated artifacts and QA data are not frontend inputs.
      ignored: ["**/target/**", "**/src-tauri/**", "**/research/**"],
    },
  },
  // Avoid walking the Cargo output tree to discover HTML entry points.
  optimizeDeps: {
    entries: ["index.html"],
  },
});
