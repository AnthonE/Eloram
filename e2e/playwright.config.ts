import { defineConfig } from "@playwright/test";

// The server is started by scripts/e2e.sh (it needs a different mode per suite).
// In the dev container Chromium comes from /opt/pw-browsers (PLAYWRIGHT_BROWSERS_PATH); never
// `playwright install` there.
const common = ["--no-proxy-server", "--autoplay-policy=no-user-gesture-required"];

export default defineConfig({
  testDir: "./tests",
  // Headless Chromium draws in software (SwiftShader), well under a frame a second on the map's
  // art (docs/ART.md): a whole match's walk-through needs the time.
  timeout: 300_000,
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
      // WebGPU through SwiftShader's Vulkan, headless: Playwright's headless shell offers an
      // adapter with these (a headed browser under Xvfb doesn't). The page needs a secure
      // context, which 127.0.0.1 is.
      name: "webgpu",
      use: {
        browserName: "chromium",
        launchOptions: {
          args: [
            ...common,
            "--enable-unsafe-webgpu",
            "--enable-features=Vulkan",
            "--use-vulkan=swiftshader",
            "--use-webgpu-adapter=swiftshader",
            "--use-angle=swiftshader",
            "--enable-unsafe-swiftshader",
            "--ignore-gpu-blocklist",
          ],
        },
      },
    },
  ],
});
