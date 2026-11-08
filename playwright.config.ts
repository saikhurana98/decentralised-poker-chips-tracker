import { defineConfig } from "@playwright/test";
import { join } from "node:path";
import { tmpdir } from "node:os";
export default defineConfig({
  testDir: "./test",
  testMatch: "browser.spec.ts",
  workers: 1,
  retries: 0,
  timeout: 30000,
  use: { baseURL: "http://localhost:18083", headless: true },
  webServer: {
    command: "./target/debug/poker-bank",
    url: "http://localhost:18083/healthz",
    timeout: 20000,
    reuseExistingServer: false,
    env: {
      PORT: "18083",
      PUBLIC_ORIGIN: "http://localhost:18083",
      DB_PATH: join(
        tmpdir(),
        `poker-browser-${String(process.pid)}`,
        "test.db",
      ),
    },
  },
});
