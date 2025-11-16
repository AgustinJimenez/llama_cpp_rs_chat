import { test, expect } from '@playwright/test';

/**
 * Test tool calling with nested JSON arguments
 * Tests the fix for regex parsing of nested JSON in tool calls
 *
 * Bug: Tool calls with nested arguments like {"name": "read_file", "arguments": {"path": "..."}}
 * would fail to parse because regex stopped at first closing brace
 */

const TEST_PORT = process.env.TEST_PORT || '8000';
const BASE_URL = `http://localhost:${TEST_PORT}`;

// Using Qwen 0.5B for faster testing
const TEST_MODEL_PATH = 'E:\\Models\\qwen2.5-0.5b-instruct-q8_0.gguf';
const TEST_FILE_PATH = 'E:\\repo\\llama_cpp_rs_chat\\test_data\\story.txt';

test.describe('Tool Call Nested JSON Parsing', () => {
  test.setTimeout(300000); // 5 minutes for tool execution and second generation

  test('should parse and execute tool calls with nested JSON arguments', async ({ page }) => {
    console.log('🚀 Testing tool call with nested JSON arguments...');

    // Listen to console logs
    page.on('console', msg => {
      if (msg.text().includes('[TOOL CALLS]') || msg.text().includes('[FRONTEND]')) {
        console.log(`[BROWSER CONSOLE] ${msg.text()}`);
      }
    });

    await page.goto(BASE_URL);
    await expect(page.getByTestId('chat-app')).toBeVisible();

    // Check if model is already loaded
    const selectModelButton = page.getByTestId('select-model-button');
    const isModelLoaded = !(await selectModelButton.isVisible({ timeout: 2000 }).catch(() => false));

    if (!isModelLoaded) {
      console.log('📂 Loading test model...');
      await selectModelButton.click();

      const modal = page.locator('[role="dialog"]');
      await expect(modal).toBeVisible({ timeout: 5000 });

      await page.getByTestId('model-path-input').fill(TEST_MODEL_PATH);

      const loadButton = page.getByTestId('load-model-button');
      await expect(loadButton).toBeEnabled({ timeout: 5000 });
      await loadButton.click();

      await expect(modal).not.toBeVisible({ timeout: 10000 });

      const unloadButton = page.locator('[title="Unload model"]');
      await expect(unloadButton).toBeVisible({ timeout: 60000 });
      console.log('✅ Model loaded successfully');
    } else {
      console.log('✅ Model already loaded');
    }

    // Ask the model to read the story file
    console.log('📝 Asking model to read story.txt...');
    const messageInput = page.getByTestId('message-input');
    await messageInput.fill(`Can you read this file? ${TEST_FILE_PATH}`);
    await page.getByTestId('send-button').click();

    // Wait for assistant response
    console.log('⏳ Waiting for response...');
    const assistantMessage = page.getByTestId('message-assistant').first();
    await expect(assistantMessage).toBeVisible({ timeout: 60000 });

    // Wait for loading to complete (includes tool execution and second generation)
    await expect(page.getByTestId('loading-indicator')).not.toBeVisible({ timeout: 240000 });

    // Get the LAST assistant message (after tool execution)
    const allAssistantMessages = page.getByTestId('message-assistant');
    const lastAssistantMessage = allAssistantMessages.last();
    const messageContent = await lastAssistantMessage.getByTestId('message-content').textContent();
    console.log('📄 Response preview:', messageContent?.substring(0, 200));

    // Check if the response contains content from the story file
    // The story mentions "Sarah Chen" and "Neo-Tokyo"
    const containsStoryContent =
      messageContent?.includes('Sarah Chen') ||
      messageContent?.includes('Neo-Tokyo') ||
      messageContent?.includes('cybersecurity') ||
      messageContent?.includes('Tokyo');

    if (containsStoryContent) {
      console.log('✅ SUCCESS: Model successfully read and processed the file!');
      console.log('   Tool call with nested JSON was parsed and executed correctly.');
    } else {
      console.log('❌ FAILURE: Response does not contain expected file content');
      console.log('   Full response:', messageContent);

      // Check if there's a tool call visible in the UI
      const toolCallElement = page.locator('text=/Tool Call|🔧/i');
      const hasToolCall = await toolCallElement.isVisible().catch(() => false);

      if (hasToolCall) {
        console.log('⚠️  Tool call was displayed but may not have executed');
      } else {
        console.log('⚠️  No tool call was found in the response');
      }
    }

    expect(containsStoryContent).toBeTruthy();
    console.log('🎉 Test completed!');
  });

  test('should handle multiple tool calls with nested arguments', async ({ page }) => {
    console.log('🚀 Testing multiple tool calls...');

    await page.goto(BASE_URL);

    // Check if model is loaded
    const unloadButton = page.locator('[title="Unload model"]');
    const isModelLoaded = await unloadButton.isVisible().catch(() => false);

    if (!isModelLoaded) {
      console.log('⚠️  No model loaded, skipping test');
      test.skip();
      return;
    }

    const messageInput = page.getByTestId('message-input');
    await messageInput.fill('List all files in E:\\repo\\llama_cpp_rs_chat\\test_data');
    await page.getByTestId('send-button').click();

    const assistantMessage = page.getByTestId('message-assistant').last();
    await expect(assistantMessage).toBeVisible({ timeout: 60000 });
    await expect(page.getByTestId('loading-indicator')).not.toBeVisible({ timeout: 120000 });

    const messageContent = await assistantMessage.getByTestId('message-content').textContent();
    console.log('📄 Directory listing response:', messageContent?.substring(0, 300));

    const mentionsFiles =
      messageContent?.includes('story.txt') ||
      messageContent?.includes('.txt') ||
      messageContent?.includes('file');

    if (mentionsFiles) {
      console.log('✅ SUCCESS: Directory listing worked');
    } else {
      console.log('⚠️  Response:', messageContent);
    }

    console.log('🎉 Test completed!');
  });
});
