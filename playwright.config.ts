import { defineConfig, devices } from '@playwright/test';

/**
 * @see https://playwright.dev/docs/test-configuration
 */
export default defineConfig({
  testDir: './tests/e2e',
  /* Run tests in files in parallel */
  fullyParallel: true,
  /* Fail the build on CI if you accidentally left test.only in the source code. */
  forbidOnly: !!process.env.CI,
  /* Retry on CI only */
  retries: process.env.CI ? 2 : 0,
  /* Use single worker to prevent concurrent model loading issues */
  /* The backend can only load one model at a time, parallel loading causes crashes */
  workers: 1,
  /* Reporter to use. See https://playwright.dev/docs/test-reporters */
  reporter: [['list'], ['json', { outputFile: 'test-results.json' }]],
  /* Shared settings for all the projects below. See https://playwright.dev/docs/api/class-testoptions. */
  use: {
    /* Base URL to use in actions like `await page.goto('/')`. */
    baseURL: 'http://localhost:14000',

    /* Collect trace when retrying the failed test. See https://playwright.dev/docs/trace-viewer */
    trace: 'on-first-retry',

    /* Take screenshot on failure */
    screenshot: 'only-on-failure',
  },

  /* Configure projects for major browsers */
  projects: [
    {
      name: 'chromium',
      use: {
        ...devices['Desktop Chrome'],
        // On CI, use pre-installed system Chrome (avoids 170MB Playwright download)
        ...(process.env.CI ? { channel: 'chrome' } : {}),
      },
    },
  ],

  /* On CI, serve the built frontend with vite preview.
     Locally, tests expect the dev server to already be running. */
  webServer: process.env.CI
    ? {
        command: 'npx vite preview --port 14000',
        port: 14000,
        reuseExistingServer: !process.env.CI,
      }
    : undefined,
});
