import { defineConfig, devices } from "@playwright/test";

// The server is started by run.sh (ephemeral, temp DB); BASE + ADMIN_LINK come
// from the environment.
export default defineConfig({
  testDir: ".",
  testMatch: "**/*.spec.ts",
  fullyParallel: false,
  forbidOnly: true,
  reporter: "list",
  use: {
    baseURL: process.env.BASE || "http://127.0.0.1:3990",
    headless: true,
    trace: "retain-on-failure",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
});
