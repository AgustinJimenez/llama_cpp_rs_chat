import { test, expect } from '@playwright/test';

const TEST_PORT = process.env.TEST_PORT || '8000';
const BASE_URL = `http://localhost:${TEST_PORT}`;

test.describe('Conversation Loading - Simple Test', () => {
  test('should load an existing conversation from sidebar without crashing', async ({ page }) => {
    // Go to the app
    await page.goto(BASE_URL);

    // Wait for page to load
    await expect(page.getByTestId('chat-app')).toBeVisible({ timeout: 5000 });

    console.log('✓ App loaded');

    // Open sidebar
    const sidebarToggle = page.getByTestId('sidebar-toggle');
    await expect(sidebarToggle).toBeVisible({ timeout: 5000 });
    await sidebarToggle.click();

    console.log('✓ Sidebar opened');

    // Wait for conversations list
    await expect(page.getByTestId('conversations-list')).toBeVisible({ timeout: 5000 });

    // Check if there are any conversations
    const firstConversation = page.getByTestId('conversation-0');
    const hasConversations = await firstConversation.isVisible({ timeout: 2000 }).catch(() => false);

    if (!hasConversations) {
      console.log('⚠️  No conversations found - test requires existing conversation');
      test.skip();
      return;
    }

    console.log('✓ Found conversations in sidebar');

    // Click on first conversation
    await firstConversation.click();

    console.log('✓ Clicked on conversation');

    // Wait a bit to see if app crashes
    await page.waitForTimeout(2000);

    // Check if the app is still responsive (no crash)
    await expect(page.getByTestId('chat-app')).toBeVisible();

    // Check if messages loaded (may or may not have messages depending on conversation)
    const messages = page.locator('[data-testid^="message-"]');
    const messageCount = await messages.count();

    console.log(`✓ App did not crash! Found ${messageCount} messages`);

    // Check for any error toasts or system errors
    const errorMessage = page.locator('text=/error|crash|fail/i');
    const hasError = await errorMessage.isVisible({ timeout: 1000 }).catch(() => false);

    if (hasError) {
      const errorText = await errorMessage.textContent();
      console.log(`❌ Error detected: ${errorText}`);
      throw new Error(`App showed error: ${errorText}`);
    }

    console.log('✓ No errors detected');
    console.log('✅ Test passed: Conversation loaded successfully without crashing');
  });
});
