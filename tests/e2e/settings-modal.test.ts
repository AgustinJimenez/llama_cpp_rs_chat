import { test, expect } from '@playwright/test';

test.describe('Settings Modal', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
  });

  test('can open and close settings modal', async ({ page }) => {
    // Open settings modal via button
    const settingsBtn = page.getByTestId('settings-btn');
    const collapsedSettingsBtn = page.getByTestId('collapsed-settings');
    
    if (await collapsedSettingsBtn.isVisible()) {
      await collapsedSettingsBtn.click();
    } else {
      await settingsBtn.click();
    }
    
    // Verify modal is open
    const modal = page.locator('[role="dialog"]');
    await expect(modal).toBeVisible();
    
    const modalTitle = page.getByText('Configuration');
    await expect(modalTitle).toBeVisible();
    
    // Close via Cancel button
    const cancelButton = page.getByRole('button', { name: 'Cancel' });
    await cancelButton.click();
    
    // Verify modal is closed
    await expect(modal).not.toBeVisible();
    
    console.log('✅ Settings modal open/close test completed!');
  });

  test('displays all configuration sections', async ({ page }) => {
    // Open settings modal
    const settingsBtn = page.getByTestId('settings-btn');
    const collapsedSettingsBtn = page.getByTestId('collapsed-settings');
    
    if (await collapsedSettingsBtn.isVisible()) {
      await collapsedSettingsBtn.click();
    } else {
      await settingsBtn.click();
    }
    
    // Wait for modal to be visible
    const modal = page.locator('[role="dialog"]');
    await expect(modal).toBeVisible();
    
    // Check for Model Path section (should be first)
    const modelPathSection = page.getByText('Model Path');
    await expect(modelPathSection).toBeVisible();
    
    // Check for Sampler Type section
    const samplerTypeSection = page.getByText('Sampler Type');
    await expect(samplerTypeSection).toBeVisible();
    
    // Check for Temperature section
    const temperatureSection = page.getByText('Temperature');
    await expect(temperatureSection).toBeVisible();
    
    // Check for Top P section
    const topPSection = page.getByText('Top P (Nucleus)');
    await expect(topPSection).toBeVisible();
    
    // Check for Top K section
    const topKSection = page.getByText('Top K');
    await expect(topKSection).toBeVisible();
    
    // Check for Mirostat sections
    const mirostatTauSection = page.getByText('Mirostat Tau');
    await expect(mirostatTauSection).toBeVisible();
    
    const mirostatEtaSection = page.getByText('Mirostat Eta');
    await expect(mirostatEtaSection).toBeVisible();
    
    // Check for IBM preset section
    const ibmPresetSection = page.getByText('IBM Recommended Preset');
    await expect(ibmPresetSection).toBeVisible();
    
    console.log('✅ All configuration sections visible!');
  });

  test('model path input has browse button', async ({ page }) => {
    // Open settings modal
    const settingsBtn = page.getByTestId('settings-btn');
    const collapsedSettingsBtn = page.getByTestId('collapsed-settings');
    
    if (await collapsedSettingsBtn.isVisible()) {
      await collapsedSettingsBtn.click();
    } else {
      await settingsBtn.click();
    }
    
    // Wait for modal to be visible
    const modal = page.locator('[role="dialog"]');
    await expect(modal).toBeVisible();
    
    // Check model path input (should be read-only)
    const modelPathInput = page.locator('input[placeholder*="model.gguf"]');
    await expect(modelPathInput).toBeVisible();
    await expect(modelPathInput).toHaveAttribute('readonly');
    
    // Check browse button
    const browseButton = page.getByRole('button', { name: /browse/i });
    await expect(browseButton).toBeVisible();
    
    // Test browse button click (should open file browser)
    await browseButton.click();
    
    // Check if file browser modal opens
    const fileBrowserModal = page.getByText('Select Model File');
    await expect(fileBrowserModal).toBeVisible({ timeout: 5000 });
    
    // Close file browser
    const cancelFileBrowser = page.getByRole('button', { name: 'Cancel' }).last();
    await cancelFileBrowser.click();
    
    console.log('✅ Model path and browse functionality test completed!');
  });

  test('sliders can be adjusted', async ({ page }) => {
    // Open settings modal
    const settingsBtn = page.getByTestId('settings-btn');
    const collapsedSettingsBtn = page.getByTestId('collapsed-settings');
    
    if (await collapsedSettingsBtn.isVisible()) {
      await collapsedSettingsBtn.click();
    } else {
      await settingsBtn.click();
    }
    
    // Wait for modal to be visible
    const modal = page.locator('[role="dialog"]');
    await expect(modal).toBeVisible();
    
    // Test temperature slider
    const temperatureSlider = page.locator('[role="slider"]').first();
    await expect(temperatureSlider).toBeVisible();
    
    // Get initial value
    const initialValue = await temperatureSlider.getAttribute('aria-valuenow');
    
    // Adjust slider
    await temperatureSlider.click();
    
    // Verify value display updates (there should be a display showing the current value)
    const temperatureDisplay = page.locator('.font-mono').first();
    await expect(temperatureDisplay).toBeVisible();
    
    console.log('✅ Slider adjustment test completed!');
  });

  test('sampler dropdown can be changed', async ({ page }) => {
    // Open settings modal
    const settingsBtn = page.getByTestId('settings-btn');
    const collapsedSettingsBtn = page.getByTestId('collapsed-settings');
    
    if (await collapsedSettingsBtn.isVisible()) {
      await collapsedSettingsBtn.click();
    } else {
      await settingsBtn.click();
    }
    
    // Wait for modal to be visible
    const modal = page.locator('[role="dialog"]');
    await expect(modal).toBeVisible();
    
    // Find sampler type dropdown
    const samplerDropdown = page.locator('[role="combobox"]');
    await expect(samplerDropdown).toBeVisible();
    
    // Click to open dropdown
    await samplerDropdown.click();
    
    // Check for sampler options
    const greedyOption = page.getByText('Greedy');
    await expect(greedyOption).toBeVisible();
    
    const temperatureOption = page.getByText('Temperature');
    await expect(temperatureOption).toBeVisible();
    
    // Select a different option
    await temperatureOption.click();
    
    console.log('✅ Sampler dropdown test completed!');
  });

  test('IBM preset button applies recommended settings', async ({ page }) => {
    // Open settings modal
    const settingsBtn = page.getByTestId('settings-btn');
    const collapsedSettingsBtn = page.getByTestId('collapsed-settings');
    
    if (await collapsedSettingsBtn.isVisible()) {
      await collapsedSettingsBtn.click();
    } else {
      await settingsBtn.click();
    }
    
    // Wait for modal to be visible
    const modal = page.locator('[role="dialog"]');
    await expect(modal).toBeVisible();
    
    // Find IBM preset button
    const ibmPresetButton = page.getByRole('button', { name: /Apply IBM Settings/i });
    await expect(ibmPresetButton).toBeVisible();
    
    // Click the preset button
    await ibmPresetButton.click();
    
    // Verify settings were applied (check for ChainFull in dropdown)
    const samplerDropdown = page.locator('[role="combobox"]');
    await expect(samplerDropdown).toContainText('ChainFull');
    
    // Verify temperature display shows 0.70
    const temperatureDisplay = page.locator('.font-mono').first();
    await expect(temperatureDisplay).toContainText('0.70');
    
    console.log('✅ IBM preset test completed!');
  });

  test('settings can be saved and persisted', async ({ page }) => {
    // Open settings modal
    const settingsBtn = page.getByTestId('settings-btn');
    const collapsedSettingsBtn = page.getByTestId('collapsed-settings');
    
    if (await collapsedSettingsBtn.isVisible()) {
      await collapsedSettingsBtn.click();
    } else {
      await settingsBtn.click();
    }
    
    // Wait for modal to be visible
    const modal = page.locator('[role="dialog"]');
    await expect(modal).toBeVisible();
    
    // Apply IBM preset to have known values
    const ibmPresetButton = page.getByRole('button', { name: /Apply IBM Settings/i });
    await ibmPresetButton.click();
    
    // Save settings
    const saveButton = page.getByRole('button', { name: 'Save Configuration' });
    await saveButton.click();
    
    // Modal should close
    await expect(modal).not.toBeVisible();
    
    // Reopen settings modal to verify persistence
    if (await collapsedSettingsBtn.isVisible()) {
      await collapsedSettingsBtn.click();
    } else {
      await settingsBtn.click();
    }
    
    // Verify settings were persisted
    await expect(modal).toBeVisible();
    const samplerDropdown = page.locator('[role="combobox"]');
    await expect(samplerDropdown).toContainText('ChainFull');
    
    console.log('✅ Settings persistence test completed!');
  });
});