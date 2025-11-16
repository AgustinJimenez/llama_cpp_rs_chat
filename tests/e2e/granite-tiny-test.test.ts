import { test, expect } from '@playwright/test';

test.describe('Granite Tiny Model Test', () => {
  test('loads Granite tiny model and generates a simple response', async ({ page }) => {
    // Increase test timeout for model loading
    test.setTimeout(300000); // 5 minutes total timeout

    // Step 1: Navigate to the app
    console.log('📱 Navigating to the app...');
    await page.goto('http://localhost:8000', { waitUntil: 'networkidle' });

    // Verify the main app loaded
    await expect(page.getByTestId('chat-app')).toBeVisible();
    console.log('✅ App loaded successfully');

    // Step 2: Check if model is already loaded
    const unloadButton = page.locator('[title="Unload model"]');
    const isModelAlreadyLoaded = await unloadButton.isVisible().catch(() => false);

    if (isModelAlreadyLoaded) {
      console.log('🔄 Model already loaded, unloading first...');
      await unloadButton.click();
      await page.waitForTimeout(2000);
    }

    // Step 3: Open model configuration modal
    console.log('🔧 Opening model configuration modal...');
    const selectModelButton = page.getByTestId('select-model-button');
    await expect(selectModelButton).toBeVisible();
    await selectModelButton.click();

    // Wait for modal
    const modal = page.locator('[role="dialog"]');
    await expect(modal).toBeVisible({ timeout: 10000 });
    console.log('✅ Model configuration modal opened');

    // Step 4: Enter Granite model path
    const modelPath = 'E:\\.lmstudio\\lmstudio-community\\granite-4.0-h-tiny-GGUF\\granite-4.0-h-tiny-Q8_0.gguf';
    console.log('📝 Selecting Granite tiny model:', modelPath);

    // Check if model is in history
    const historyPath = page.locator('button').filter({ hasText: 'granite-4.0-h-tiny-Q8_0.gguf' });
    const hasHistory = await historyPath.count() > 0;

    if (hasHistory) {
      console.log('📋 Found model in history, clicking on it...');
      await historyPath.first().click();
    } else {
      console.log('✏️  Model not in history, typing path manually...');
      const modelPathInput = page.getByTestId('model-path-input');
      await expect(modelPathInput).toBeVisible();
      await modelPathInput.click();
      await modelPathInput.fill(modelPath);
    }

    // Wait for file validation
    console.log('⏳ Waiting for file validation...');
    await page.waitForTimeout(2000);

    const fileExistsIndicator = page.getByTestId('file-found-label');
    await expect(fileExistsIndicator).toBeVisible({ timeout: 10000 });
    console.log('✅ Model file found and validated');

    // Step 5: Load the model
    console.log('🚀 Loading Granite model...');
    const loadButton = page.getByRole('button', { name: /Load Model/i });
    await expect(loadButton).toBeVisible();
    await loadButton.click();

    // Wait for model to load (look for success message or modal close)
    console.log('⏳ Waiting for model to load...');
    await page.waitForTimeout(5000); // Give it time to load

    // Check if modal closed (indicates success)
    const modalClosed = await modal.isVisible().then(visible => !visible).catch(() => true);
    if (modalClosed) {
      console.log('✅ Modal closed - model loaded successfully');
    } else {
      // Look for success indicator
      const successMessage = page.getByText(/Model loaded successfully|Loaded/i);
      await expect(successMessage).toBeVisible({ timeout: 30000 });
      console.log('✅ Model loaded successfully');
    }

    // Step 6: Send a simple test message
    console.log('💬 Sending test message...');
    const messageInput = page.locator('textarea[placeholder*="message" i], textarea[placeholder*="type" i]').first();
    await expect(messageInput).toBeVisible();

    const testMessage = 'Hello! Please respond with a short greeting.';
    await messageInput.fill(testMessage);
    await messageInput.press('Enter');

    // Wait for response
    console.log('⏳ Waiting for response...');
    await page.waitForTimeout(10000); // Wait for generation

    // Check that we got a response
    const messages = page.locator('.message-bubble');
    const messageCount = await messages.count();
    console.log(`📊 Total messages: ${messageCount}`);

    // Should have at least 2 messages (user + assistant)
    expect(messageCount).toBeGreaterThanOrEqual(2);

    // Get the last message (assistant's response)
    const lastMessage = await messages.last().textContent();
    console.log('🤖 Assistant response:', lastMessage?.substring(0, 200));

    // Verify we got a response
    expect(lastMessage).toBeTruthy();
    expect(lastMessage!.length).toBeGreaterThan(5);

    console.log('✅ Granite tiny model test passed!');
  });
});
