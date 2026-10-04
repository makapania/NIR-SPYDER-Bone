import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// Tauri expects a fixed dev port and must not clear the terminal it shares with cargo.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  // fs.allow: this folder plus the one repository file the Help dialog bundles (USER_GUIDE.md), never the whole
  // repository (planning/ and data/ are private)
  server: { port: 5173, strictPort: true, fs: { allow: ['.', '../../USER_GUIDE.md'] } },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  build: {
    // WebView2 is evergreen Chromium; macOS 13 ships Safari 16 WebKit.
    target: ['es2020', 'chrome105', 'safari16'],
    sourcemap: false,
  },
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node',
  },
});
