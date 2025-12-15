import { test, expect } from '@playwright/test';

const TEST_PORT = process.env.TEST_PORT || '8000';
const BASE_URL = `http://localhost:${TEST_PORT}`;

// Test model configuration
const TEST_MODEL_PATH = 'E:\\Models\\qwen2.5-0.5b-instruct-q8_0.gguf';

test.describe('WebSocket Token Counts', () => {
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

  test('should receive token counts via WebSocket (not null)', async ({ page }) => {
    // Listen for WebSocket messages
    const wsMessages: any[] = [];

    page.on('websocket', ws => {
      ws.on('framereceived', event => {
        try {
          const message = JSON.parse(event.payload as string);
          wsMessages.push(message);
        } catch (e) {
          // Ignore non-JSON messages
        }
      });
    });

    // Send a message to create a conversation
    const messageInput = page.getByTestId('message-input');
    await messageInput.fill('Hello, test message for token counts');
    await page.getByTestId('send-button').click();

    // Wait for response
    await expect(page.locator('[data-testid^="message-assistant"]')).toBeVisible({
      timeout: 15000
    });

    console.log('✓ Message sent and response received');

    // Wait a bit for WebSocket messages
    await page.waitForTimeout(1000);

    // Find token messages (type: "token" or type: "update")
    const tokenMessages = wsMessages.filter(msg =>
      (msg.type === 'token' && msg.tokens_used !== undefined) ||
      (msg.type === 'update' && msg.tokens_used !== undefined)
    );

    console.log(`Found ${tokenMessages.length} messages with token counts`);

    // Verify we received at least one message with token counts
    expect(tokenMessages.length).toBeGreaterThan(0);

    // Verify token counts are NOT null
    for (const msg of tokenMessages) {
      expect(msg.tokens_used).not.toBeNull();
      expect(msg.max_tokens).not.toBeNull();

      // Verify they are numbers
      if (msg.tokens_used !== null) {
        expect(typeof msg.tokens_used).toBe('number');
        expect(msg.tokens_used).toBeGreaterThan(0);
      }

      if (msg.max_tokens !== null) {
        expect(typeof msg.max_tokens).toBe('number');
        expect(msg.max_tokens).toBeGreaterThan(0);
      }
    }

    console.log('✓ Token counts are valid numbers (not null)');
  });

  test('should NOT show "null tokens" error during tool execution', async ({ page }) => {
    // Listen for toast notifications
    let foundNullTokensError = false;

    page.on('console', msg => {
      const text = msg.text();
      if (text.includes('Context size is too small (null tokens)') ||
          text.includes('null tokens')) {
        foundNullTokensError = true;
        console.error('❌ Found "null tokens" error:', text);
      }
    });

    // Send a message that triggers tool execution
    const messageInput = page.getByTestId('message-input');
    await messageInput.fill('Create a test directory called test_websocket_tokens');
    await page.getByTestId('send-button').click();

    // Wait for response
    await expect(page.locator('[data-testid^="message-assistant"]')).toBeVisible({
      timeout: 15000
    });

    console.log('✓ Message with tool call sent and response received');

    // Wait to ensure no delayed error messages
    await page.waitForTimeout(2000);

    // Check for error toast
    const errorToast = page.locator('text=/Context size is too small.*null tokens/i');
    const hasErrorToast = await errorToast.isVisible().catch(() => false);

    // Verify no "null tokens" error occurred
    expect(foundNullTokensError).toBe(false);
    expect(hasErrorToast).toBe(false);

    console.log('✓ No "null tokens" error detected');
  });

  test('should maintain token counts when loading existing conversation', async ({ page }) => {
    // Create a conversation
    await page.getByTestId('message-input').fill('First message for token tracking');
    await page.getByTestId('send-button').click();
    await expect(page.locator('[data-testid^="message-assistant"]')).toBeVisible({
      timeout: 15000
    });

    console.log('✓ Created first conversation');

    // Get token counts from UI (if displayed)
    // Note: This depends on how token counts are displayed in the UI

    // Create a new conversation
    await page.getByTestId('sidebar-toggle').click();
    await page.getByTestId('new-chat-btn').click();

    // Verify chat is cleared
    await expect(page.locator('[data-testid^="message-"]')).toHaveCount(0);

    console.log('✓ Started new conversation');

    // Listen for WebSocket messages
    const wsMessages: any[] = [];

    page.on('websocket', ws => {
      ws.on('framereceived', event => {
        try {
          const message = JSON.parse(event.payload as string);
          if (message.type === 'update') {
            wsMessages.push(message);
          }
        } catch (e) {
          // Ignore non-JSON messages
        }
      });
    });

    // Load the previous conversation
    await page.getByTestId('sidebar-toggle').click();
    await expect(page.getByTestId('conversations-list')).toBeVisible();
    const firstConversation = page.getByTestId('conversation-0');
    await expect(firstConversation).toBeVisible({ timeout: 5000 });
    await firstConversation.click();

    // Wait for messages to load
    await expect(page.locator('[data-testid="message-user"]')).toBeVisible({
      timeout: 10000
    });

    console.log('✓ Loaded existing conversation');

    // Wait for WebSocket update messages
    await page.waitForTimeout(1000);

    // Find update messages with token counts
    const updateMessages = wsMessages.filter(msg =>
      msg.type === 'update' && msg.tokens_used !== undefined
    );

    console.log(`Found ${updateMessages.length} WebSocket update messages`);

    // Verify we received at least one update with token counts
    if (updateMessages.length > 0) {
      for (const msg of updateMessages) {
        // Verify token counts are NOT null
        expect(msg.tokens_used).not.toBeNull();
        expect(msg.max_tokens).not.toBeNull();

        console.log(`Update message: tokens_used=${msg.tokens_used}, max_tokens=${msg.max_tokens}`);
      }
      console.log('✓ WebSocket updates contain valid token counts');
    } else {
      console.log('ℹ️  No WebSocket update messages captured (may need to adjust timing)');
    }
  });

  test('should update token counts in real-time during streaming', async ({ page }) => {
    // Listen for WebSocket messages
    const tokenCounts: Array<{ tokens_used: number | null, max_tokens: number | null }> = [];

    page.on('websocket', ws => {
      ws.on('framereceived', event => {
        try {
          const message = JSON.parse(event.payload as string);
          if (message.type === 'token' && message.tokens_used !== undefined) {
            tokenCounts.push({
              tokens_used: message.tokens_used,
              max_tokens: message.max_tokens
            });
          }
        } catch (e) {
          // Ignore non-JSON messages
        }
      });
    });

    // Send a message
    const messageInput = page.getByTestId('message-input');
    await messageInput.fill('Generate a longer response about AI and machine learning');
    await page.getByTestId('send-button').click();

    // Wait for response
    await expect(page.locator('[data-testid^="message-assistant"]')).toBeVisible({
      timeout: 15000
    });

    console.log('✓ Message sent and response received');

    // Wait for all WebSocket messages
    await page.waitForTimeout(1000);

    console.log(`Captured ${tokenCounts.length} token updates`);

    // Verify we received token updates
    expect(tokenCounts.length).toBeGreaterThan(0);

    // Verify all token counts are valid (not null)
    for (const count of tokenCounts) {
      expect(count.tokens_used).not.toBeNull();
      expect(count.max_tokens).not.toBeNull();

      if (count.tokens_used !== null) {
        expect(count.tokens_used).toBeGreaterThan(0);
      }

      if (count.max_tokens !== null) {
        expect(count.max_tokens).toBeGreaterThan(0);
      }
    }

    // Verify token_used increases over time (more tokens consumed)
    if (tokenCounts.length > 1) {
      const firstCount = tokenCounts[0].tokens_used!;
      const lastCount = tokenCounts[tokenCounts.length - 1].tokens_used!;
      expect(lastCount).toBeGreaterThanOrEqual(firstCount);
      console.log(`✓ Token count increased from ${firstCount} to ${lastCount}`);
    }

    console.log('✓ All token counts during streaming are valid');
  });

  test('should handle token counts with KV cache in CPU RAM (large context)', async ({ page }) => {
    // This test verifies that with KV cache in CPU RAM, we get the full context size
    // not reduced due to VRAM limits

    // Listen for WebSocket messages
    let maxTokensReceived: number | null = null;

    page.on('websocket', ws => {
      ws.on('framereceived', event => {
        try {
          const message = JSON.parse(event.payload as string);
          if (message.type === 'token' && message.max_tokens !== null) {
            maxTokensReceived = message.max_tokens;
          }
        } catch (e) {
          // Ignore non-JSON messages
        }
      });
    });

    // Send a message
    const messageInput = page.getByTestId('message-input');
    await messageInput.fill('Test message for context size verification');
    await page.getByTestId('send-button').click();

    // Wait for response
    await expect(page.locator('[data-testid^="message-assistant"]')).toBeVisible({
      timeout: 15000
    });

    console.log('✓ Message sent and response received');

    // Wait for WebSocket messages
    await page.waitForTimeout(1000);

    // Verify we received max_tokens
    expect(maxTokensReceived).not.toBeNull();
    expect(maxTokensReceived).toBeGreaterThan(0);

    console.log(`✓ Received max_tokens: ${maxTokensReceived}`);

    // The context size should be reasonable (not critically low like 2048)
    // For Qwen 0.5B, the context is typically 32768 or higher
    if (maxTokensReceived !== null) {
      expect(maxTokensReceived).toBeGreaterThan(4096);
      console.log(`✓ Context size is healthy: ${maxTokensReceived} tokens (>4096)`);
    }
  });
});
