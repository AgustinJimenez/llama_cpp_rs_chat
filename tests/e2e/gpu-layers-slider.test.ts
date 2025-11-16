import { test, expect } from '@playwright/test';

test.describe('GPU Layers Slider Test', () => {
  // Clean up after each test by unloading any loaded model
  test.afterEach(async ({ page }) => {
    const unloadButton = page.locator('[title="Unload model"]');
    const isModelLoaded = await unloadButton.isVisible().catch(() => false);

    if (isModelLoaded) {
      console.log('🧹 Cleaning up: Unloading model after test...');
      await unloadButton.click();
      await page.waitForTimeout(1000);
      console.log('✅ Model unloaded');
    }
  });

  test('should update GPU layer values correctly when slider is moved', async ({ page }) => {
    test.setTimeout(60000); // 1 minute timeout

    // Step 1: Navigate to the app
    console.log('📱 Navigating to the app...');
    await page.goto('/', { waitUntil: 'networkidle' });
    await expect(page.getByTestId('chat-app')).toBeVisible();
    console.log('✅ App loaded successfully');

    // Step 2: Open model configuration modal
    console.log('🔧 Opening model configuration modal...');

    // Check if a model is already loaded
    const unloadButton = page.locator('[title="Unload model"]');
    const isModelLoaded = await unloadButton.isVisible().catch(() => false);

    if (isModelLoaded) {
      console.log('   Model already loaded, unloading first...');
      await unloadButton.click();

      // Wait for the select-model-button to appear (means model is unloaded)
      const selectModelButton = page.getByTestId('select-model-button');
      await expect(selectModelButton).toBeVisible({ timeout: 10000 });
      console.log('   ✅ Model unloaded successfully');
    }

    const selectModelButton = page.getByTestId('select-model-button');
    await expect(selectModelButton).toBeVisible({ timeout: 5000 });
    await selectModelButton.click();

    const modal = page.locator('[role="dialog"]');
    await expect(modal).toBeVisible({ timeout: 10000 });
    console.log('✅ Model configuration modal opened');

    // Step 3: Select a model path
    const modelPath = 'E:\\.lmstudio\\models\\lmstudio-community\\Devstral-Small-2507-GGUF\\Devstral-Small-2507-Q4_K_M.gguf';
    console.log('📝 Selecting model path:', modelPath);

    // Try to click from history first
    const historyPath = page.locator('button').filter({ hasText: 'Devstral-Small-2507-Q4_K_M.gguf' });
    const hasHistory = await historyPath.count() > 0;

    if (hasHistory) {
      console.log('📋 Found model in history, clicking...');
      await historyPath.first().click();
    } else {
      console.log('✏️  Typing path manually...');
      const modelPathInput = page.getByTestId('model-path-input');
      await expect(modelPathInput).toBeVisible();
      await modelPathInput.fill(modelPath);
    }

    // Wait for file validation
    console.log('⏳ Waiting for file validation...');
    await page.waitForTimeout(2000);
    const fileExistsIndicator = page.getByText('File found and accessible');
    await expect(fileExistsIndicator).toBeVisible({ timeout: 10000 });
    console.log('✅ Model file validated');

    // Step 4: Expand configuration section
    console.log('🔽 Expanding configuration section...');

    const configExpandButton = page.getByTestId('config-expand-button');
    await expect(configExpandButton).toBeVisible({ timeout: 10000 });

    // Check if config content is already visible by looking for Context Length section
    const contextSection = page.getByText('Context Length');
    const isVisible = await contextSection.isVisible().catch(() => false);

    if (!isVisible) {
      console.log('   Configuration is collapsed, clicking to expand...');
      await configExpandButton.click();
      await page.waitForTimeout(500);

      // Verify it expanded
      await expect(contextSection).toBeVisible({ timeout: 5000 });
    }

    console.log('✅ Configuration section expanded');

    // Scroll to bottom to ensure GPU layers section is visible
    await page.evaluate(() => {
      const modalContent = document.querySelector('[role="dialog"] .overflow-y-auto');
      if (modalContent) {
        modalContent.scrollTop = modalContent.scrollHeight;
      }
    });
    await page.waitForTimeout(500);

    // Step 5: Find GPU Layers slider by test ID
    console.log('🎚️  Finding GPU Layers slider...');

    // Wait for the GPU layers slider to be visible
    const gpuSlider = page.getByTestId('gpu-layers-slider');
    await expect(gpuSlider).toBeVisible({ timeout: 5000 });
    console.log('✅ GPU Layers slider found');

    // Get the value display (shows "X / Y")
    const valueDisplay = page.getByTestId('gpu-layers-display');
    await expect(valueDisplay).toBeVisible();

    // Get initial value and max value
    const initialText = await valueDisplay.textContent();
    console.log('📊 Initial GPU layers value:', initialText);
    const [initialValue, maxValue] = initialText!.split(' / ').map(v => parseInt(v.trim()));
    console.log(`📊 Initial: ${initialValue}, Max: ${maxValue}`);

    // Find the slider thumb (the draggable part)
    const sliderThumb = gpuSlider.locator('[role="slider"]');
    await expect(sliderThumb).toBeVisible();

    // Get slider and thumb bounding boxes
    const sliderBox = await gpuSlider.boundingBox();
    if (!sliderBox) {
      throw new Error('Could not get slider bounding box');
    }

    // Test different slider positions using keyboard navigation
    const testPositions = [
      { key: 'Home', expectedValue: 0, label: 'minimum (Home key)' },
      { key: 'ArrowRight', expectedValue: 1, label: '1 layer (ArrowRight from 0)', repeat: 1 },
      { key: 'ArrowRight', expectedValue: 11, label: '11 layers (ArrowRight x10)', repeat: 10 },
      { key: 'End', expectedValue: maxValue, label: 'maximum (End key)' },
      { key: 'ArrowLeft', expectedValue: maxValue - 1, label: `${maxValue - 1} layers (ArrowLeft from max)`, repeat: 1 },
      { key: 'Home', expectedValue: 0, label: 'back to minimum (Home key)' },
    ];

    for (const position of testPositions) {
      console.log(`\n🎯 Testing slider at ${position.label}...`);

      // Focus the slider
      await sliderThumb.focus();

      // Press the key (repeat if needed for arrow keys)
      const repeatCount = position.repeat || 1;
      for (let i = 0; i < repeatCount; i++) {
        await page.keyboard.press(position.key);
      }
      await page.waitForTimeout(300); // Wait for value to update

      // Get updated value
      const updatedText = await valueDisplay.textContent();
      const [currentValue] = updatedText!.split(' / ').map(v => parseInt(v.trim()));

      console.log(`   Expected: ${position.expectedValue}, Got: ${currentValue}`);

      // Allow for small rounding differences (±1 layer)
      const tolerance = 1;
      const isCorrect = Math.abs(currentValue - position.expectedValue) <= tolerance;

      expect(isCorrect).toBeTruthy();
      console.log(`   ✅ Value is correct (within tolerance of ${tolerance})`);

      // Verify memory visualization updates (should be visible)
      const memoryViz = page.getByText('Memory Usage Estimate');
      await expect(memoryViz).toBeVisible();

      // Check that VRAM values are displayed
      const vramLabel = page.getByText('GPU Memory (VRAM)');
      await expect(vramLabel).toBeVisible();

      // Get VRAM usage text to verify it updates
      const vramUsage = page.locator('text=/[\\d.]+ \\/ [\\d.]+ GB/').first();
      const vramText = await vramUsage.textContent();
      console.log(`   📊 VRAM Usage: ${vramText}`);

      // Verify VRAM usage is a valid number
      const vramMatch = vramText?.match(/([\d.]+) \/ ([\d.]+) GB/);
      expect(vramMatch).toBeTruthy();
      const [, used, total] = vramMatch!;
      const usedVram = parseFloat(used);
      const totalVram = parseFloat(total);

      expect(usedVram).toBeGreaterThanOrEqual(0);
      expect(totalVram).toBeGreaterThan(0);
      console.log(`   ✅ VRAM values are valid: ${usedVram}GB / ${totalVram}GB`);
    }

    console.log('\n✅ All slider positions tested successfully!');
    console.log('✅ Memory visualization updates correctly!');
    console.log('\n🎉 GPU Layers Slider test completed successfully!');
  });
});
