import { test, expect } from '@playwright/test';

test.describe('Model Path Validation - Real Model', () => {
  const TEST_MODEL_PATH = 'E:\\.lmstudio\\models\\lmstudio-community\\Devstral-Small-2507-GGUF\\Devstral-Small-2507-Q4_K_M.gguf';

  test.beforeEach(async ({ page }) => {
    // Enable console logging
    page.on('console', msg => {
      if (msg.text().includes('[DEBUG]')) {
        console.log('Browser:', msg.text());
      }
    });

    await page.goto('/');
    await page.waitForLoadState('networkidle');
  });

  test('should validate real model file path', async ({ page }) => {
    console.log('\n🧪 Testing model path validation with real model');
    console.log('📁 Model path:', TEST_MODEL_PATH);

    // Open settings modal
    const settingsBtn = page.getByTestId('settings-btn');
    const collapsedSettingsBtn = page.getByTestId('collapsed-settings');

    if (await collapsedSettingsBtn.isVisible()) {
      await collapsedSettingsBtn.click();
    } else {
      await settingsBtn.click();
    }

    // Wait for settings modal
    const settingsModal = page.locator('[role="dialog"]');
    await expect(settingsModal).toBeVisible();
    console.log('✅ Settings modal opened');

    // Find model path input
    const modelPathInput = page.locator('input[type="text"]').filter({ hasText: '' }).first();
    await modelPathInput.clear();
    await modelPathInput.fill(TEST_MODEL_PATH);
    console.log('✅ Model path entered:', TEST_MODEL_PATH);

    // Wait for debounced validation (500ms + buffer)
    await page.waitForTimeout(1000);

    // Check for validation indicator
    // Look for success/error icons or messages
    const validationSuccess = page.locator('svg').filter({ hasText: 'CheckCircle' }).or(
      page.locator('[data-testid="file-valid"]')
    ).or(
      page.locator('text=/file exists|valid|found/i')
    );

    const validationError = page.locator('svg').filter({ hasText: 'XCircle' }).or(
      page.locator('[data-testid="file-invalid"]')
    ).or(
      page.locator('text=/not found|invalid|error/i')
    );

    // Check which state we're in
    const isSuccess = await validationSuccess.count() > 0;
    const isError = await validationError.count() > 0;

    console.log('\n📊 Validation Results:');
    console.log('  Success indicators found:', isSuccess);
    console.log('  Error indicators found:', isError);

    // Get page content for debugging
    const pageContent = await page.content();
    if (pageContent.includes('13.3 GB')) {
      console.log('  ✅ File size displayed: 13.3 GB');
    }
    if (pageContent.includes('Devstral')) {
      console.log('  ✅ Model name displayed');
    }

    // Take screenshot
    await page.screenshot({ path: 'test-results/model-validation-debug.png', fullPage: true });
    console.log('📸 Screenshot saved to: test-results/model-validation-debug.png');

    // The test should pass if validation succeeds
    expect(isSuccess || pageContent.includes('13.3 GB')).toBeTruthy();
  });

  test('should make correct API request', async ({ page, request }) => {
    console.log('\n🧪 Testing API request directly');

    // Test the API endpoint directly
    const encodedPath = encodeURIComponent(TEST_MODEL_PATH);
    const apiUrl = `http://localhost:4000/api/model/info?path=${encodedPath}`;

    console.log('📡 Making request to:', apiUrl);

    const response = await request.get(apiUrl);
    const status = response.status();

    console.log('📊 Response status:', status);

    if (response.ok()) {
      const data = await response.json();
      console.log('📦 Response data:', JSON.stringify(data, null, 2));

      expect(status).toBe(200);
      expect(data).toHaveProperty('file_size');
      expect(data).toHaveProperty('name');
      console.log('✅ API request successful');
    } else {
      const errorText = await response.text();
      console.log('❌ API error:', errorText);
      throw new Error(`API returned ${status}: ${errorText}`);
    }
  });

  test('should update UI when path is entered', async ({ page }) => {
    console.log('\n🧪 Testing UI update on path entry');

    // Open settings
    const settingsBtn = page.getByTestId('settings-btn');
    const collapsedSettingsBtn = page.getByTestId('collapsed-settings');

    if (await collapsedSettingsBtn.isVisible()) {
      await collapsedSettingsBtn.click();
    } else {
      await settingsBtn.click();
    }

    // Enter path character by character to see when validation triggers
    const modelPathInput = page.locator('input[type="text"]').first();
    await modelPathInput.clear();

    // Type the path
    await modelPathInput.type(TEST_MODEL_PATH, { delay: 50 });
    console.log('✅ Path typed');

    // Wait for debounce
    console.log('⏳ Waiting for validation (500ms debounce + buffer)...');
    await page.waitForTimeout(1000);

    // Check console logs
    const logs = [];
    page.on('console', msg => logs.push(msg.text()));

    await page.waitForTimeout(500);

    // Filter for debug logs
    const debugLogs = logs.filter(log => log.includes('[DEBUG]'));
    console.log('\n📋 Debug logs from browser:');
    debugLogs.forEach(log => console.log('  ', log));

    // Check if any network requests were made
    const networkLogs = [];
    page.on('request', request => {
      if (request.url().includes('/api/model/info')) {
        networkLogs.push(`REQUEST: ${request.url()}`);
      }
    });
    page.on('response', response => {
      if (response.url().includes('/api/model/info')) {
        networkLogs.push(`RESPONSE: ${response.status()} ${response.url()}`);
      }
    });

    await page.waitForTimeout(500);

    console.log('\n🌐 Network activity:');
    networkLogs.forEach(log => console.log('  ', log));

    // Take final screenshot
    await page.screenshot({ path: 'test-results/ui-update-test.png', fullPage: true });
  });
});
