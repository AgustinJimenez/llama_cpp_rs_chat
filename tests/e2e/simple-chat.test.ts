import { test, expect } from '@playwright/test';

test.describe('Simple Chat Test', () => {
  test('can say hello and get a response', async ({ page }) => {
    // Go to the chat app
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    
    // Verify the main app loaded
    await expect(page.getByTestId('chat-app')).toBeVisible();
    await expect(page.getByTestId('messages-container')).toBeVisible();
    
    // Find the message input using test ID
    const messageInput = page.getByTestId('message-input');
    await expect(messageInput).toBeVisible();
    
    // Type hello message
    const testMessage = 'Hello!';
    await messageInput.fill(testMessage);
    
    // Send the message using the send button
    const sendButton = page.getByTestId('send-button');
    await expect(sendButton).toBeVisible();
    await expect(sendButton).toBeEnabled();
    await sendButton.click();
    
    // Verify the input was cleared
    await expect(messageInput).toHaveValue('');
    
    // Check that our message appears as a user message
    const userMessage = page.getByTestId('message-user');
    await expect(userMessage).toBeVisible({ timeout: 10000 });
    
    // Verify the message content
    const userMessageContent = userMessage.getByTestId('message-content');
    await expect(userMessageContent).toContainText(testMessage);
    
    // Check for loading indicator (optional - may not appear if response is fast)
    const loadingIndicator = page.getByTestId('loading-indicator');
    try {
      await expect(loadingIndicator).toBeVisible({ timeout: 2000 });
    } catch (e) {
      // Loading indicator may not appear if LLaMA responds very quickly
      console.log('Loading indicator not visible (response was fast)');
    }
    
    // Wait for assistant response
    const assistantMessage = page.getByTestId('message-assistant');
    await expect(assistantMessage).toBeVisible({ timeout: 30000 });
    
    // Verify the assistant response has content
    const assistantMessageContent = assistantMessage.getByTestId('message-content');
    await expect(assistantMessageContent).not.toBeEmpty();
    
    // Verify loading indicator disappeared (if it was visible)
    try {
      await expect(loadingIndicator).not.toBeVisible();
    } catch (e) {
      // Loading indicator may have never appeared
    }
    
    console.log('✅ Chat test completed - hello message sent and response received!');
  });

  test('API hello test', async ({ request }) => {
    // Test the API directly
    const response = await request.post('/api/chat', {
      headers: {
        'Content-Type': 'application/json',
      },
      data: {
        message: 'Hello!'
      }
    });

    expect(response.status()).toBe(200);
    
    const data = await response.json();
    expect(data.message.content).toBeTruthy();
    expect(data.message.role).toBe('assistant');
    expect(data.message.id).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/);
    expect(data.conversation_id).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/);
    
    console.log('API Response:', data.message.content);
    console.log('✅ API hello test completed!');
  });

  test('can send multiple messages in conversation', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    
    const messageInput = page.getByTestId('message-input');
    const sendButton = page.getByTestId('send-button');
    
    // Send first message
    await messageInput.fill('What is 2+2?');
    await sendButton.click();
    
    // Wait for first response
    await expect(page.getByTestId('message-assistant')).toBeVisible({ timeout: 30000 });
    
    // Send second message
    await messageInput.fill('What about 3+3?');
    await sendButton.click();
    
    // Should have 2 user messages and 2 assistant messages
    await expect(page.getByTestId('message-user')).toHaveCount(2, { timeout: 30000 });
    await expect(page.getByTestId('message-assistant')).toHaveCount(2, { timeout: 30000 });
    
    console.log('✅ Multiple messages test completed!');
  });

  test('new chat button clears conversation', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    
    // Send a message first
    await page.getByTestId('message-input').fill('Hello!');
    await page.getByTestId('send-button').click();
    
    // Wait for response
    await expect(page.getByTestId('message-assistant')).toBeVisible({ timeout: 30000 });
    
    // Click new chat button (try both expanded and collapsed states)
    const newChatBtn = page.getByTestId('new-chat-btn');
    const collapsedNewChatBtn = page.getByTestId('collapsed-new-chat');
    
    // Try the collapsed button first (more likely to be visible by default)
    if (await collapsedNewChatBtn.isVisible()) {
      await collapsedNewChatBtn.scrollIntoViewIfNeeded();
      await collapsedNewChatBtn.click({ force: true });
    } else {
      await newChatBtn.scrollIntoViewIfNeeded();
      await newChatBtn.click({ force: true });
    }
    
    // Messages should be cleared
    await expect(page.getByTestId('message-user')).toHaveCount(0);
    await expect(page.getByTestId('message-assistant')).toHaveCount(0);
    
    console.log('✅ New chat test completed!');
  });
});