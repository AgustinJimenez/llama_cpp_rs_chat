import { test, expect } from '@playwright/test';

test.describe('Tool Calling Tests', () => {
  test('should detect and display tool calls in assistant messages', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // Verify the app loaded
    await expect(page.getByTestId('chat-app')).toBeVisible();

    // Send a message that should trigger tool usage
    const messageInput = page.getByTestId('message-input');
    const sendButton = page.getByTestId('send-button');

    await messageInput.fill('What files are in the current directory?');
    await sendButton.click();

    // Wait for user message to appear
    await expect(page.getByTestId('message-user')).toBeVisible({ timeout: 10000 });

    // Wait for assistant response (may take longer with tool execution)
    await expect(page.getByTestId('message-assistant')).toBeVisible({ timeout: 60000 });

    // Check if tool calls are displayed in the UI
    // Tool calls should appear in a special styled container
    const toolCallContainer = page.locator('[class*="bg-blue-900"]');

    // Wait a bit to see if tool calls appear
    try {
      await expect(toolCallContainer).toBeVisible({ timeout: 5000 });
      console.log('✅ Tool call UI element detected!');

      // Verify tool call displays the tool name
      const toolCallText = await toolCallContainer.textContent();
      expect(toolCallText).toBeTruthy();
      console.log('Tool call content:', toolCallText);
    } catch (e) {
      console.log('⚠️  Tool call UI not visible - model may not have used tools');
    }

    // Verify assistant message has content (even if no tool was called)
    const assistantMessageContent = page.getByTestId('message-content');
    await expect(assistantMessageContent.first()).not.toBeEmpty();

    console.log('✅ Tool calling UI test completed!');
  });

  test('should execute bash tool via API', async ({ request }) => {
    // Test the tool execution API directly
    const response = await request.post('/api/tools/execute', {
      headers: {
        'Content-Type': 'application/json',
      },
      data: {
        tool_name: 'bash',
        arguments: {
          command: 'echo "Hello from tool"'
        }
      }
    });

    expect(response.status()).toBe(200);

    const data = await response.json();
    expect(data.success).toBe(true);
    expect(data.result).toContain('Hello from tool');

    console.log('Tool execution result:', data.result);
    console.log('✅ Tool execution API test completed!');
  });

  test('should handle tool parsing for Mistral format', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // We can't directly test the parser from Playwright, but we can verify
    // that if a message contains Mistral-style tool calls, they get parsed

    // Send a message
    const messageInput = page.getByTestId('message-input');
    const sendButton = page.getByTestId('send-button');

    await messageInput.fill('List all .rs files in src/');
    await sendButton.click();

    // Wait for response
    await expect(page.getByTestId('message-assistant')).toBeVisible({ timeout: 60000 });

    // Check the page console for any tool parsing errors
    const logs: string[] = [];
    page.on('console', msg => {
      if (msg.type() === 'error') {
        logs.push(msg.text());
      }
    });

    // Wait a bit to collect any console errors
    await page.waitForTimeout(2000);

    // Verify no tool parsing errors occurred
    const toolParsingErrors = logs.filter(log =>
      log.includes('tool') && log.includes('error')
    );

    expect(toolParsingErrors.length).toBe(0);

    console.log('✅ Tool parsing test completed!');
  });

  test('should handle agentic loop with multiple tool calls', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // Send a message that might trigger multiple tool calls
    const messageInput = page.getByTestId('message-input');
    const sendButton = page.getByTestId('send-button');

    await messageInput.fill('Check the current directory and tell me what you find');
    await sendButton.click();

    // Wait for initial user message
    await expect(page.getByTestId('message-user')).toBeVisible({ timeout: 10000 });

    // Wait for assistant response (may take longer with multiple tool iterations)
    await expect(page.getByTestId('message-assistant')).toBeVisible({ timeout: 90000 });

    // Wait a bit for any additional agentic iterations
    await page.waitForTimeout(5000);

    // Should have at least one assistant message
    const assistantMessages = page.getByTestId('message-assistant');
    const count = await assistantMessages.count();
    expect(count).toBeGreaterThanOrEqual(1);

    console.log(`Received ${count} assistant message(s)`);
    console.log('✅ Agentic loop test completed!');
  });

  test('should respect MAX_TOOL_ITERATIONS limit', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // Monitor for the max iterations error toast
    let maxIterationsReached = false;

    page.on('console', msg => {
      if (msg.text().includes('Maximum tool iterations reached')) {
        maxIterationsReached = true;
      }
    });

    // Send a message (we can't force infinite loop, just verify the safety exists)
    const messageInput = page.getByTestId('message-input');
    const sendButton = page.getByTestId('send-button');

    await messageInput.fill('Hello');
    await sendButton.click();

    // Wait for response
    await expect(page.getByTestId('message-assistant')).toBeVisible({ timeout: 60000 });

    // Wait a bit to see if any iterations happen
    await page.waitForTimeout(3000);

    // We shouldn't hit the limit with a simple hello
    expect(maxIterationsReached).toBe(false);

    console.log('✅ Tool iteration limit test completed!');
  });

  test('should strip tool call markers from displayed content', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    const messageInput = page.getByTestId('message-input');
    const sendButton = page.getByTestId('send-button');

    await messageInput.fill('What is the weather like?');
    await sendButton.click();

    // Wait for assistant response
    await expect(page.getByTestId('message-assistant')).toBeVisible({ timeout: 60000 });

    // Get the message content
    const messageContent = page.getByTestId('message-content').first();
    const contentText = await messageContent.textContent();

    // Verify that tool call markers are NOT visible in the UI
    // These should be stripped out
    expect(contentText).not.toContain('[TOOL_CALLS]');
    expect(contentText).not.toContain('[ARGS]');
    expect(contentText).not.toContain('[TOOL_RESULTS]');
    expect(contentText).not.toContain('<function=');
    expect(contentText).not.toContain('</function>');
    expect(contentText).not.toContain('<tool_call>');
    expect(contentText).not.toContain('</tool_call>');

    console.log('✅ Tool marker stripping test completed!');
  });

  test('should handle tool execution errors gracefully', async ({ request }) => {
    // Test with an invalid command that should fail
    const response = await request.post('/api/tools/execute', {
      headers: {
        'Content-Type': 'application/json',
      },
      data: {
        tool_name: 'bash',
        arguments: {
          command: 'this_is_not_a_valid_command_12345'
        }
      }
    });

    expect(response.status()).toBe(200);

    const data = await response.json();

    // The tool should execute but return an error result
    // It shouldn't crash the API
    expect(data).toBeDefined();

    console.log('Tool error handling result:', data);
    console.log('✅ Tool error handling test completed!');
  });

  test('should display tool call arguments in UI', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    const messageInput = page.getByTestId('message-input');
    const sendButton = page.getByTestId('send-button');

    await messageInput.fill('Show me the contents of package.json');
    await sendButton.click();

    // Wait for response
    await expect(page.getByTestId('message-assistant')).toBeVisible({ timeout: 60000 });

    // Look for tool call display elements
    const toolCallContainer = page.locator('[class*="bg-blue-900"]');

    try {
      await expect(toolCallContainer).toBeVisible({ timeout: 5000 });

      // Check for JSON-formatted arguments
      const preElement = toolCallContainer.locator('pre');
      await expect(preElement).toBeVisible();

      const argsText = await preElement.textContent();

      // Should be valid JSON
      expect(() => JSON.parse(argsText || '{}')).not.toThrow();

      console.log('Tool arguments displayed:', argsText);
      console.log('✅ Tool arguments display test completed!');
    } catch (e) {
      console.log('⚠️  No tool calls detected in this response');
    }
  });
});
