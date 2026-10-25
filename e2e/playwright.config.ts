import { defineConfig } from "@playwright/test";

// The server is started by scripts/e2e.sh (it needs a different mode per suite).
// In the dev container Chromium comes from /opt/pw-browsers (PLAYWRIGHT_BROWSERS_PATH); never
// `playwright install` there.
const common = ["--no-proxy-server", "--autoplay-policy=no-user-gesture-required"];

export default defineConfig({
  testDir: "./tests",
  timeout: 180_000,
  retries: 0,
  reporter: process.env.CI ? [["list"], ["html", { open: "never" }]] : [["list"]],
  outputDir: "./test-results",
  use: {
    baseURL: process.env.EL_URL ?? "http://127.0.0.1:8080",
    viewport: { width: 960, height: 540 },
    trace: "retain-on-failure",
  },
  projects: [
    {
      // Required: WebGL2 via SwiftShader works headless everywhere.
      name: "webgl2",
      use: {
        browserName: "chromium",
        launchOptions: {
          args: [...common, "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
        },
      },
    },
    {
      // Best-effort: WebGPU needs a headed browser (run under xvfb-run) and SwiftShader Vulkan.
      name: "webgpu",
      use: {
        browserName: "chromium",
        headless: false,
        launchOptions: {
          args: [
            ...common,
            "--enable-unsafe-webgpu",
            "--enable-features=Vulkan",
            "--use-vulkan=swiftshader",
            "--use-webgpu-adapter=swiftshader",
            "--use-angle=vulkan",
            "--disable-vulkan-surface",
          ],
        },
      },
    },
  ],
});
