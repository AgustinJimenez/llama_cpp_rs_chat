import { test, expect } from '@playwright/test';
import fs from 'fs';
import path from 'path';

const DEFAULT_MODEL_PATH = 'E:/.lmstudio/models/lmstudio-community/Devstral-Small-2507-GGUF/Devstral-Small-2507-Q4_K_M.gguf';
const MODEL_PATH = process.env.TEST_MODEL_PATH || DEFAULT_MODEL_PATH;

test.describe('Main Flow - Load model and send message', () => {
  test('loads model and responds to hello', async ({ page }) => {
    test.setTimeout(180_000);
    const resolvedPath = path.resolve(MODEL_PATH);
    if (!fs.existsSync(resolvedPath)) {
      test.skip(`Model file not found at ${resolvedPath}`);
    }

    // Open app
    await page.goto('/');

    // Open model selector
    await page.getByTestId('select-model-button').click();

    // Fill model path
    await page.getByTestId('model-path-input').fill(resolvedPath);

    // Load model
    await page.getByTestId('load-model-button').click();

    // Wait for model to load by polling status
    let loaded = false;
    for (let i = 0; i < 24; i++) {
      const statusResp = await page.request.get('/api/model/status', { timeout: 5_000 });
      if (statusResp.ok()) {
        const status = await statusResp.json();
        if (status.loaded) {
          loaded = true;
          break;
        }
      }
      await page.waitForTimeout(5_000);
    }
    expect(loaded).toBeTruthy();

    // Send hello
    await page.getByTestId('message-input').fill('Hello!');
    await page.getByTestId('send-button').click();

    // Wait for assistant response
    const assistantMessage = page.getByTestId('message-assistant').last();
    await expect(assistantMessage).toContainText(/hello/i, { timeout: 60_000 });
  });
});
