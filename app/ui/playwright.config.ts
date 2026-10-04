import { defineConfig, devices } from '@playwright/test';

// Smoke tests run against the Vite dev server with the mocked api.ts (no Tauri, no core).
// Local runs can use the installed Edge (PW_CHANNEL=msedge) instead of downloading Chromium.
const channel = process.env.PW_CHANNEL;

export default defineConfig({
  testDir: 'e2e',
  timeout: 60_000,
  use: {
    baseURL: 'http://localhost:5173',
    viewport: { width: 1440, height: 900 },
    ...(channel ? { channel } : {}),
  },
  webServer: {
    command: 'npm run dev',
    url: 'http://localhost:5173',
    reuseExistingServer: true,
    timeout: 60_000,
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 900 }, ...(channel ? { channel } : {}) }, testIgnore: /screens/ },
    // WebKit stands in for macOS WKWebView on CI (03_architecture §8.4).
    { name: 'webkit', use: { ...devices['Desktop Safari'], viewport: { width: 1440, height: 900 } }, testIgnore: /screens/ },
    { name: 'screens', use: { viewport: { width: 1440, height: 900 }, ...(channel ? { channel } : {}) }, testMatch: /screens/ },
  ],
});
