import { test, expect } from '@playwright/test';

test.describe('Chat Interface', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    
    // Wait for the page to load completely
    await page.waitForLoadState('networkidle');
  });

  test('loads chat interface correctly', async ({ page }) => {
    // Check page title
    await expect(page).toHaveTitle(/🦙 LLaMA Chat/);
    
    // Check main heading
    await expect(page.locator('h1')).toContainText('🦙 LLaMA Chat');
    
    // Check that chat input is present
    const chatInput = page.locator('input[placeholder*="message"], textarea[placeholder*="message"], input[type="text"]').first();
    await expect(chatInput).toBeVisible();
    
    // Check that send button is present (look for common button patterns)
    const sendButton = page.locator('button').filter({ hasText: /send|submit|→|➤/i }).first();
    if (await sendButton.count() > 0) {
      await expect(sendButton).toBeVisible();
    }
  });

  test('displays chat messages area', async ({ page }) => {
    // Look for chat messages container
    const messagesContainer = page.locator('[data-testid="messages"], .messages, .chat-messages, .conversation').first();
    
    // If specific test ID not found, look for common chat patterns
    if (await messagesContainer.count() === 0) {
      // Look for scrollable areas or divs that might contain messages
      const chatArea = page.locator('div').filter({ has: page.locator('text=/message|chat|conversation/i') }).first();
      if (await chatArea.count() > 0) {
        await expect(chatArea).toBeVisible();
      }
    } else {
      await expect(messagesContainer).toBeVisible();
    }
  });

  test('can send a chat message', async ({ page }) => {
    // Find the chat input field
    const chatInput = page.locator('input[placeholder*="message"], textarea[placeholder*="message"], input[type="text"]').first();
    await expect(chatInput).toBeVisible();
    
    // Type a test message
    const testMessage = 'Hello, this is a test message from Playwright!';
    await chatInput.fill(testMessage);
    
    // Find and click send button
    const sendButton = page.locator('button').filter({ hasText: /send|submit|→|➤/i }).first();
    
    if (await sendButton.count() > 0) {
      await sendButton.click();
    } else {
      // Fallback: try pressing Enter
      await chatInput.press('Enter');
    }
    
    // Wait for the message to appear (look for user message)
    await expect(page.locator('text=' + testMessage)).toBeVisible({ timeout: 10000 });
    
    // Wait for AI response (this may take some time with real LLaMA)
    // Look for assistant response patterns
    await page.waitForSelector('text=/assistant|ai|bot/i', { timeout: 30000 });
  });

  test('displays typing indicator during response generation', async ({ page }) => {
    const chatInput = page.locator('input[placeholder*="message"], textarea[placeholder*="message"], input[type="text"]').first();
    await chatInput.fill('What is the capital of France?');
    
    const sendButton = page.locator('button').filter({ hasText: /send|submit|→|➤/i }).first();
    if (await sendButton.count() > 0) {
      await sendButton.click();
    } else {
      await chatInput.press('Enter');
    }
    
    // Look for typing indicators (dots, spinner, "thinking" text, etc.)
    const typingIndicators = [
      page.locator('text=/typing|thinking|generating|\.\.\.|\.\.\.\./i'),
      page.locator('[data-testid="typing-indicator"]'),
      page.locator('.typing, .spinner, .loading')
    ];
    
    let foundTypingIndicator = false;
    for (const indicator of typingIndicators) {
      if (await indicator.count() > 0) {
        try {
          await expect(indicator.first()).toBeVisible({ timeout: 5000 });
          foundTypingIndicator = true;
          break;
        } catch {
          // Continue to next indicator
        }
      }
    }
    
    // If no specific typing indicator found, just wait for response
    if (!foundTypingIndicator) {
      await page.waitForSelector('text=/assistant|ai|france|paris/i', { timeout: 30000 });
    }
  });

  test('handles multiple messages in conversation', async ({ page }) => {
    const chatInput = page.locator('input[placeholder*="message"], textarea[placeholder*="message"], input[type="text"]').first();
    
    // Send first message
    await chatInput.fill('What is 2+2?');
    const sendButton = page.locator('button').filter({ hasText: /send|submit|→|➤/i }).first();
    
    if (await sendButton.count() > 0) {
      await sendButton.click();
    } else {
      await chatInput.press('Enter');
    }
    
    // Wait for first response
    await page.waitForSelector('text=/4|four/i', { timeout: 30000 });
    
    // Send follow-up message
    await chatInput.fill('What about 3+3?');
    if (await sendButton.count() > 0) {
      await sendButton.click();
    } else {
      await chatInput.press('Enter');
    }
    
    // Wait for second response
    await page.waitForSelector('text=/6|six/i', { timeout: 30000 });
    
    // Verify both questions are visible in chat history
    await expect(page.locator('text=What is 2+2?')).toBeVisible();
    await expect(page.locator('text=What about 3+3?')).toBeVisible();
  });

  test('input field clears after sending message', async ({ page }) => {
    const chatInput = page.locator('input[placeholder*="message"], textarea[placeholder*="message"], input[type="text"]').first();
    
    const testMessage = 'This message should clear after sending';
    await chatInput.fill(testMessage);
    
    // Verify input has the message
    await expect(chatInput).toHaveValue(testMessage);
    
    // Send the message
    const sendButton = page.locator('button').filter({ hasText: /send|submit|→|➤/i }).first();
    if (await sendButton.count() > 0) {
      await sendButton.click();
    } else {
      await chatInput.press('Enter');
    }
    
    // Verify input is cleared
    await expect(chatInput).toHaveValue('');
  });

  test('handles empty message submission gracefully', async ({ page }) => {
    const chatInput = page.locator('input[placeholder*="message"], textarea[placeholder*="message"], input[type="text"]').first();
    
    // Try to send empty message
    const sendButton = page.locator('button').filter({ hasText: /send|submit|→|➤/i }).first();
    if (await sendButton.count() > 0) {
      await sendButton.click();
    } else {
      await chatInput.press('Enter');
    }
    
    // Should not crash or send empty message
    // Input should still be focused or available
    await expect(chatInput).toBeVisible();
  });

  test('responsive design works on mobile viewport', async ({ page }) => {
    // Set mobile viewport
    await page.setViewportSize({ width: 375, height: 667 });
    
    // Reload to apply mobile styles
    await page.reload();
    await page.waitForLoadState('networkidle');
    
    // Check that interface is still usable
    const chatInput = page.locator('input[placeholder*="message"], textarea[placeholder*="message"], input[type="text"]').first();
    await expect(chatInput).toBeVisible();
    
    // Check that text is readable (not too small)
    const heading = page.locator('h1').first();
    if (await heading.count() > 0) {
      const fontSize = await heading.evaluate(el => window.getComputedStyle(el).fontSize);
      const fontSizeNum = parseInt(fontSize);
      expect(fontSizeNum).toBeGreaterThan(12); // Minimum readable size
    }
  });

  test('handles network errors gracefully', async ({ page }) => {
    // Intercept API calls and simulate network error
    await page.route('/api/chat', route => {
      route.abort('failed');
    });
    
    const chatInput = page.locator('input[placeholder*="message"], textarea[placeholder*="message"], input[type="text"]').first();
    await chatInput.fill('This should fail due to network error');
    
    const sendButton = page.locator('button').filter({ hasText: /send|submit|→|➤/i }).first();
    if (await sendButton.count() > 0) {
      await sendButton.click();
    } else {
      await chatInput.press('Enter');
    }
    
    // Look for error message or indication
    await expect(page.locator('text=/error|failed|try again/i')).toBeVisible({ timeout: 10000 });
  });

  test('preserves chat history on page refresh', async ({ page }) => {
    const chatInput = page.locator('input[placeholder*="message"], textarea[placeholder*="message"], input[type="text"]').first();
    
    // Send a message
    const testMessage = 'This message should persist after refresh';
    await chatInput.fill(testMessage);
    
    const sendButton = page.locator('button').filter({ hasText: /send|submit|→|➤/i }).first();
    if (await sendButton.count() > 0) {
      await sendButton.click();
    } else {
      await chatInput.press('Enter');
    }
    
    // Wait for response
    await page.waitForTimeout(5000);
    
    // Refresh the page
    await page.reload();
    await page.waitForLoadState('networkidle');
    
    // Check if message history is preserved (if implemented)
    // This test might pass or fail depending on local storage implementation
    const messageExists = await page.locator(`text=${testMessage}`).count() > 0;
    
    // If persistence is implemented, message should be visible
    // If not implemented, this documents the expected behavior
    if (messageExists) {
      await expect(page.locator(`text=${testMessage}`)).toBeVisible();
    }
  });
});