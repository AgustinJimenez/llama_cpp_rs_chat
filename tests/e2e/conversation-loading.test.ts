import { test, expect } from '@playwright/test';

const TEST_PORT = process.env.TEST_PORT || '8000';
const BASE_URL = `http://localhost:${TEST_PORT}`;

// Test model configuration
const TEST_MODEL_PATH = 'E:\\Models\\qwen2.5-0.5b-instruct-q8_0.gguf';

test.describe('Conversation Loading', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(BASE_URL);

    // Check if a model is already loaded
    const selectModelButton = page.getByTestId('select-model-button');
    const isModelLoaded = !(await selectModelButton.isVisible({ timeout: 2000 }).catch(() => false));

    if (!isModelLoaded) {
      // Load model if not already loaded
      console.log('No model loaded, loading test model...');
      await selectModelButton.click();

      // Wait for modal to open
      const modal = page.locator('[role="dialog"]');
      await expect(modal).toBeVisible({ timeout: 5000 });

      // Fill in model path
      await page.getByTestId('model-path-input').fill(TEST_MODEL_PATH);

      // Click load button
      const loadButton = page.getByTestId('load-model-button');
      await expect(loadButton).toBeEnabled({ timeout: 5000 });
      await loadButton.click();

      // Wait for modal to close
      await expect(modal).not.toBeVisible({ timeout: 10000 });

      // Wait for model to load (unload button appears when loaded)
      const unloadButton = page.locator('[title="Unload model"]');
      await expect(unloadButton).toBeVisible({ timeout: 30000 });

      console.log('✓ Model loaded successfully');
    } else {
      console.log('✓ Model already loaded');
    }
  });

  test('should create and load a conversation from sidebar', async ({ page }) => {
    // Send a message to create a conversation
    const messageInput = page.getByTestId('message-input');
    await messageInput.fill('Hello, this is my first message');
    await page.getByTestId('send-button').click();

    // Wait for response
    await expect(page.locator('[data-testid^="message-assistant"]')).toBeVisible({
      timeout: 15000
    });

    console.log('✓ Message sent and response received');

    // Clear the chat (start new conversation)
    await page.getByTestId('sidebar-toggle').click();
    await page.getByTestId('new-chat-btn').click();

    // Verify chat is cleared
    await expect(page.locator('[data-testid^="message-"]')).toHaveCount(0);

    console.log('✓ Chat cleared');

    // Open sidebar and load the previous conversation
    await page.getByTestId('sidebar-toggle').click();

    // Wait for conversations list to load
    await expect(page.getByTestId('conversations-list')).toBeVisible();

    // Click on the first conversation
    const firstConversation = page.getByTestId('conversation-0');
    await expect(firstConversation).toBeVisible({ timeout: 5000 });

    console.log('✓ Conversation found in sidebar, clicking to load it...');
    await firstConversation.click();

    // Wait for messages to load
    await expect(page.locator('[data-testid="message-user"]')).toBeVisible({
      timeout: 10000
    });
    await expect(page.locator('[data-testid="message-assistant"]')).toBeVisible({
      timeout: 10000
    });

    // Verify the loaded message content
    const userMessage = page.locator('[data-testid="message-user"]').first();
    await expect(userMessage).toContainText('Hello, this is my first message');

    console.log('✓ Successfully loaded conversation from sidebar');
  });

  test('should handle multiple conversations', async ({ page }) => {
    // Create first conversation
    await page.getByTestId('message-input').fill('First conversation message');
    await page.getByTestId('send-button').click();
    await expect(page.locator('[data-testid^="message-assistant"]')).toBeVisible({
      timeout: 15000
    });

    // Create second conversation
    await page.getByTestId('sidebar-toggle').click();
    await page.getByTestId('new-chat-btn').click();
    await page.getByTestId('message-input').fill('Second conversation message');
    await page.getByTestId('send-button').click();
    await expect(page.locator('[data-testid^="message-assistant"]').nth(1)).toBeVisible({
      timeout: 15000
    });

    // Open sidebar and verify we see 2 conversations
    await page.getByTestId('sidebar-toggle').click();
    await expect(page.getByTestId('conversation-0')).toBeVisible({ timeout: 5000 });
    await expect(page.getByTestId('conversation-1')).toBeVisible({ timeout: 5000 });

    // Load first conversation
    await page.getByTestId('conversation-1').click(); // Most recent is first
    await expect(page.locator('[data-testid="message-user"]').first()).toContainText('First conversation message', {
      timeout: 10000
    });

    // Load second conversation
    await page.getByTestId('sidebar-toggle').click();
    await page.getByTestId('conversation-0').click();
    await expect(page.locator('[data-testid="message-user"]').first()).toContainText('Second conversation message', {
      timeout: 10000
    });

    console.log('✓ Successfully handled multiple conversations');
  });

  test('should refresh conversations list', async ({ page }) => {
    // Create a conversation
    await page.getByTestId('message-input').fill('Test message for refresh');
    await page.getByTestId('send-button').click();
    await expect(page.locator('[data-testid^="message-assistant"]')).toBeVisible({
      timeout: 15000
    });

    // Open sidebar
    await page.getByTestId('sidebar-toggle').click();
    await expect(page.getByTestId('conversations-list')).toBeVisible();

    // Verify conversation appears
    await expect(page.getByTestId('conversation-0')).toBeVisible({ timeout: 5000 });

    // Click refresh button
    await page.getByTestId('refresh-conversations').click();

    // Wait for loading to complete
    await expect(page.getByText('Loading...')).not.toBeVisible({ timeout: 5000 });

    // Verify conversation still appears
    await expect(page.getByTestId('conversation-0')).toBeVisible();

    console.log('✓ Successfully refreshed conversations');
  });

  test('should show active conversation indicator', async ({ page }) => {
    // Create a conversation
    await page.getByTestId('message-input').fill('Active conversation test');
    await page.getByTestId('send-button').click();
    await expect(page.locator('[data-testid^="message-assistant"]')).toBeVisible({
      timeout: 15000
    });

    // Open sidebar
    await page.getByTestId('sidebar-toggle').click();

    // The current conversation should have active styling
    const activeConversation = page.getByTestId('conversation-0');
    await expect(activeConversation).toBeVisible({ timeout: 5000 });

    // Check if it has the active class (border-primary)
    const classList = await activeConversation.getAttribute('class');
    expect(classList).toContain('border-primary');

    console.log('✓ Active conversation indicator working');
  });
});
