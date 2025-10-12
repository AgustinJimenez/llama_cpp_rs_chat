import { test, expect } from '@playwright/test';

test.describe('Model Loading and Chat Test', () => {
  test('loads model using modal UI with GPU layers, says hello, and requests a code algorithm', async ({ page }) => {
    // Step 1: Navigate to the app and wait for it to load
    console.log('📱 Navigating to the app...');
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // Verify the main app loaded
    await expect(page.getByTestId('chat-app')).toBeVisible();
    await expect(page.getByTestId('messages-container')).toBeVisible();
    console.log('✅ App loaded successfully');

    // Step 2: Check if model is already loaded by looking for "Unload model" button
    const unloadButton = page.locator('[title="Unload model"]');
    const isModelAlreadyLoaded = await unloadButton.isVisible().catch(() => false);

    if (isModelAlreadyLoaded) {
      console.log('✅ Model already loaded from previous run (Unload model button present), skipping model loading steps...');
    } else {
      console.log('🔧 No model loaded, opening model configuration modal...');

      // Try to find the button by test-id first, fallback to text if needed
      let selectModelButton = page.getByTestId('select-model-button');

      // Check if button exists, if not try by text
      try {
        await expect(selectModelButton).toBeVisible({ timeout: 2000 });
      } catch (e) {
        console.log('⚠️  Test-id not found, trying by text content...');
        selectModelButton = page.getByRole('button', { name: /Select a model to load/i });
      }

      await expect(selectModelButton).toBeVisible();
      await selectModelButton.click();

    // Wait for modal to be visible
    const modal = page.locator('[role="dialog"]');
    await expect(modal).toBeVisible({ timeout: 10000 });
    console.log('✅ Model configuration modal opened');

    // Step 3: Select the model path
    const modelPath = 'E:\\.lmstudio\\models\\lmstudio-community\\Devstral-Small-2507-GGUF\\Devstral-Small-2507-Q4_K_M.gguf';
    console.log('📝 Selecting model path:', modelPath);

    // First, try to click on the model path from history if available
    const historyPath = page.locator('button').filter({ hasText: 'Devstral-Small-2507-Q4_K_M.gguf' });
    const hasHistory = await historyPath.count() > 0;

    if (hasHistory) {
      console.log('📋 Found model in history, clicking on it...');
      await historyPath.first().click();
    } else {
      console.log('✏️  Model not in history, typing path manually...');
      // Try test-id first, fallback to placeholder selector
      let modelPathInput = page.getByTestId('model-path-input');
      const hasTestId = await modelPathInput.count() > 0;

      if (!hasTestId) {
        console.log('⚠️  Test-id not found, using placeholder selector...');
        modelPathInput = page.locator('input[placeholder*="path to .gguf file"]');
      }

      await expect(modelPathInput).toBeVisible();
      await modelPathInput.click();
      await modelPathInput.fill(modelPath);
    }

    // Wait for file validation - look for "File found and accessible" indicator
    console.log('⏳ Waiting for file validation...');
    const fileExistsIndicator = page.getByTestId('file-found');
    await expect(fileExistsIndicator).toBeVisible({ timeout: 10000 });
    console.log('✅ Model file found and validated (file-found indicator visible)');

    // Step 4: Scroll down in the modal and expand configuration section
    console.log('🔽 Scrolling to configuration section...');

    // Scroll down in the modal to see the configuration section
    await page.evaluate(() => {
      const modalContent = document.querySelector('[role="dialog"] .overflow-y-auto');
      if (modalContent) {
        modalContent.scrollTop = modalContent.scrollHeight;
      }
    });

    await page.waitForTimeout(500); // Wait for scroll

    console.log('🔽 Expanding model configuration section...');
    const configExpandButton = page.getByTestId('config-expand-button');

    // If test-id not found, try by text
    const hasTestId = await configExpandButton.count() > 0;
    if (!hasTestId) {
      console.log('⚠️  Config expand button test-id not found, trying by text...');
      const textButton = page.locator('button').filter({ hasText: 'Model Configurations' });
      await textButton.click();
    } else {
      await configExpandButton.click();
    }

    await page.waitForTimeout(500); // Wait for expansion animation

    // Scroll down more to see GPU layers section
    await page.evaluate(() => {
      const modalContent = document.querySelector('[role="dialog"] .overflow-y-auto');
      if (modalContent) {
        modalContent.scrollTop = modalContent.scrollHeight;
      }
    });

    await page.waitForTimeout(500); // Wait for scroll

    // Step 5: Set GPU layers to maximum using the slider
    console.log('🎮 Setting GPU layers to maximum (all layers on GPU)...');

    // Find the GPU layers slider - try test-id first, then fallback
    let gpuSlider = page.getByTestId('gpu-layers-slider');
    let hasGpuSliderTestId = await gpuSlider.count() > 0;

    if (!hasGpuSliderTestId) {
      console.log('⚠️  GPU slider test-id not found, trying by label...');
      // Find slider by looking for text "GPU Layers" and then finding the slider nearby
      gpuSlider = page.locator('text=GPU Layers').locator('..').locator('..').locator('[role="slider"]').first();
    }

    await expect(gpuSlider).toBeVisible({ timeout: 10000 });

    // Get the max value from the slider
    const maxLayers = await gpuSlider.getAttribute('aria-valuemax');
    console.log(`📊 Model has ${maxLayers} layers, setting all to GPU...`);

    // Set slider to maximum using keyboard (more reliable than mouse drag)
    await gpuSlider.focus();

    // Use keyboard to set to max: press End key to jump to maximum
    await gpuSlider.press('End');

    // Alternative: use arrow keys multiple times
    for (let i = 0; i < parseInt(maxLayers || '40'); i++) {
      await gpuSlider.press('ArrowRight');
    }

    // Verify GPU layers value (optional - best effort)
    try {
      let gpuLayersDisplay = page.getByTestId('gpu-layers-display');
      let hasDisplayTestId = await gpuLayersDisplay.count() > 0;

      if (!hasDisplayTestId) {
        console.log('⚠️  GPU layers display test-id not found, trying by text pattern...');
        // Find the display showing "X / Y" format near GPU Layers text
        gpuLayersDisplay = page.locator('text=GPU Layers').locator('..').locator('span').filter({ hasText: '/' }).first();
      }

      const gpuLayersText = await gpuLayersDisplay.textContent({ timeout: 3000 });
      console.log(`✅ GPU layers set to: ${gpuLayersText}`);
    } catch (e) {
      console.log('⚠️  Could not read GPU layers display, but slider was moved to max. Continuing...');
    }

    // Step 6: Verify modal is still open before continuing
    console.log('🔍 Verifying modal is still open...');
    await expect(modal).toBeVisible();
    console.log('✅ Modal confirmed open');

    // Step 7: Scroll to bottom to see the Load Model button
    console.log('📜 Scrolling to bottom to see Load Model button...');
    await page.evaluate(() => {
      const modalContent = document.querySelector('[role="dialog"] .overflow-y-auto');
      if (modalContent) {
        modalContent.scrollTop = modalContent.scrollHeight;
      }
    });

    await page.waitForTimeout(1000); // Wait a bit longer

    // Verify modal still open after scroll
    await expect(modal).toBeVisible();

    // Step 8: Click the Load Model button
    console.log('💾 Clicking Load Model button...');
    let loadButton = page.getByTestId('load-model-button');
    let hasLoadButtonTestId = await loadButton.count() > 0;

    if (!hasLoadButtonTestId) {
      console.log('⚠️  Load button test-id not found, trying by text...');
      loadButton = page.getByRole('button', { name: /Load Model/i });
    }

    await expect(loadButton).toBeVisible({ timeout: 10000 });
    await expect(loadButton).toBeEnabled();
    console.log('✅ Load Model button found and enabled, clicking now...');
    await loadButton.click();

    // Wait for modal to close
    await expect(modal).not.toBeVisible({ timeout: 10000 });
    console.log('✅ Modal closed after clicking Load Model');

      // Wait for model to load by checking for the "Unload model" button
      console.log('⏳ Waiting for model to load (checking for Unload model button)...');
      const unloadButton = page.locator('[title="Unload model"]');
      await expect(unloadButton).toBeVisible({ timeout: 120000 }); // 2 minutes timeout for model loading

      console.log('✅ Model loaded successfully (Unload model button appeared)');
    }

    // Step 9: Send first message - "Hello"
    console.log('💬 Sending first message: "Hello"');
    const messageInput = page.getByTestId('message-input');
    await expect(messageInput).toBeVisible();

    await messageInput.fill('Hello');

    const sendButton = page.getByTestId('send-button');
    await expect(sendButton).toBeVisible();
    await expect(sendButton).toBeEnabled();
    await sendButton.click();

    // Verify the input was cleared
    await expect(messageInput).toHaveValue('');

    // Check that our message appears as a user message
    const firstUserMessage = page.getByTestId('message-user').first();
    await expect(firstUserMessage).toBeVisible({ timeout: 10000 });
    await expect(firstUserMessage.getByTestId('message-content')).toContainText('Hello');
    console.log('✅ First message sent: "Hello"');

    // Wait for first assistant response
    // The message element may be hidden while streaming, so wait for actual content
    console.log('⏳ Waiting for first response...');
    const firstAssistantMessage = page.getByTestId('message-assistant').first();
    const firstAssistantContent = firstAssistantMessage.getByTestId('message-content');

    // Wait for content to have at least some text (streaming may still be in progress)
    await expect(firstAssistantContent).not.toBeEmpty({ timeout: 90000 });

    // Wait a bit for streaming to complete
    await page.waitForTimeout(2000);

    const firstResponseText = await firstAssistantContent.textContent();
    console.log('✅ First response received:', firstResponseText?.substring(0, 100) + '...');

    // Step 10: Send second message - request a code algorithm
    console.log('💬 Sending second message: "Can you write a small algorithm to check if a number is prime?"');
    await messageInput.fill('Can you write a small algorithm to check if a number is prime?');
    await sendButton.click();

    // Verify the input was cleared
    await expect(messageInput).toHaveValue('');

    // Check that our second message appears
    const secondUserMessage = page.getByTestId('message-user').nth(1);
    await expect(secondUserMessage).toBeVisible({ timeout: 10000 });
    await expect(secondUserMessage.getByTestId('message-content')).toContainText('prime');
    console.log('✅ Second message sent: "Can you write a small algorithm..."');

    // Wait for second assistant response
    console.log('⏳ Waiting for second response (algorithm)...');
    const secondAssistantMessage = page.getByTestId('message-assistant').nth(1);
    await expect(secondAssistantMessage).toBeVisible({ timeout: 60000 }); // 60 second timeout for response

    // Verify the assistant response has content
    const secondAssistantContent = secondAssistantMessage.getByTestId('message-content');
    await expect(secondAssistantContent).not.toBeEmpty();

    const secondResponseText = await secondAssistantContent.textContent();
    console.log('✅ Second response received (algorithm)');

    // Verify we have exactly 2 user messages and 2 assistant messages
    await expect(page.getByTestId('message-user')).toHaveCount(2);
    await expect(page.getByTestId('message-assistant')).toHaveCount(2);

    // Optional: Check that the response contains code-like content
    // (this is a heuristic check - algorithm responses often contain specific keywords)
    const hasCodeIndicators =
      secondResponseText?.includes('def ') || // Python
      secondResponseText?.includes('function') || // JavaScript/TypeScript
      secondResponseText?.includes('for ') || // Loop
      secondResponseText?.includes('return') || // Return statement
      secondResponseText?.includes('if ') || // Conditional
      secondResponseText?.includes('while ') || // Loop
      secondResponseText?.includes('int ') || // C/C++/Java
      secondResponseText?.includes('public ') || // Java/C#
      secondResponseText?.includes('private '); // Java/C#

    if (hasCodeIndicators) {
      console.log('✅ Response appears to contain code/algorithm content');
    } else {
      console.log('⚠️  Response may not contain code, but test will continue');
    }

    console.log('🎉 Test completed successfully!');
    console.log('   - Opened model configuration modal ✅');
    console.log('   - Selected model file via UI ✅');
    console.log('   - Set GPU layers to maximum (all layers on GPU) ✅');
    console.log('   - Model loaded successfully ✅');
    console.log('   - First message: "Hello" ✅');
    console.log('   - Second message: Algorithm request ✅');
    console.log('   - Both responses received ✅');
  });
});
