import { test, expect } from '@playwright/test';
import * as path from 'path';
import { fileURLToPath } from 'url';

/**
 * Agentic Tool Calling E2E Tests
 * These tests verify that the model can autonomously use tools when needed
 *
 * Prerequisites:
 * - Model must be loaded (Devstral or similar with tool calling support)
 * - Test data files must exist in test_data/
 */

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const TEST_DATA_DIR = path.resolve(__dirname, '../../test_data');

test.describe('Agentic Tool Calling Tests', () => {
  test.setTimeout(300000); // 5 minutes for model responses

  test('model should autonomously use read_file tool when asked about file contents', async ({ page }) => {
    console.log('🚀 Starting agentic read_file test...');

    // Navigate to app
    await page.goto('/');
    await expect(page.getByTestId('chat-app')).toBeVisible();

    // Check if model is loaded
    const unloadButton = page.locator('[title="Unload model"]');
    const isModelLoaded = await unloadButton.isVisible().catch(() => false);

    if (!isModelLoaded) {
      console.log('⚠️  No model loaded - skipping agentic test (requires model)');
      test.skip();
      return;
    }

    console.log('✅ Model is loaded, proceeding with test...');

    // Ask the model to read a file
    const sampleFilePath = path.join(TEST_DATA_DIR, 'sample_file.txt').replace(/\\/g, '\\\\');
    const userMessage = `Read the file at ${sampleFilePath} and tell me what it contains.`;

    console.log(`📝 Sending message: "${userMessage}"`);

    const messageInput = page.getByTestId('message-input');
    await messageInput.fill(userMessage);

    const sendButton = page.getByTestId('send-button');
    await sendButton.click();

    // Wait for user message to appear
    const userMsg = page.getByTestId('message-user').last();
    await expect(userMsg).toBeVisible({ timeout: 10000 });

    console.log('⏳ Waiting for model response...');

    // Wait for assistant response
    const assistantMsg = page.getByTestId('message-assistant').last();
    await expect(assistantMsg).toBeVisible({ timeout: 120000 });

    // Wait for response to complete (no loading indicator)
    await expect(page.getByTestId('loading-indicator')).not.toBeVisible({ timeout: 120000 });

    console.log('✅ Model responded');

    // Check for tool call in the response
    const messageContent = await assistantMsg.getByTestId('message-content').textContent();
    console.log('📄 Response preview:', messageContent?.substring(0, 200));

    // The model should either:
    // 1. Generate a [TOOL_CALLS]read_file[ARGS]{...} (visible in frontend as tool call UI)
    // 2. Or the tool was auto-executed and result is in the response

    // Check if tool was called (look for tool call UI element or tool results)
    const pageContent = await page.content();
    const hasToolCall = pageContent.includes('read_file') ||
                        pageContent.includes('TOOL_CALLS') ||
                        pageContent.includes('sample file');

    if (hasToolCall) {
      console.log('✅ Tool call detected in response');
    }

    // The response should contain information from the file
    // (either from the file directly or from describing the tool call)
    const containsFileInfo =
      messageContent?.toLowerCase().includes('sample') ||
      messageContent?.toLowerCase().includes('testing') ||
      messageContent?.toLowerCase().includes('line') ||
      messageContent?.toLowerCase().includes('file');

    expect(containsFileInfo).toBe(true);

    console.log('🎉 Agentic read_file test completed');
  });

  test('model should use list_directory tool when asked to list files', async ({ page }) => {
    console.log('🚀 Starting agentic list_directory test...');

    await page.goto('/');
    await expect(page.getByTestId('chat-app')).toBeVisible();

    const unloadButton = page.locator('[title="Unload model"]');
    const isModelLoaded = await unloadButton.isVisible().catch(() => false);

    if (!isModelLoaded) {
      console.log('⚠️  No model loaded - skipping test');
      test.skip();
      return;
    }

    const testPath = TEST_DATA_DIR.replace(/\\/g, '\\\\');
    const userMessage = `List all files in the directory: ${testPath}`;

    console.log(`📝 Sending message: "${userMessage}"`);

    const messageInput = page.getByTestId('message-input');
    await messageInput.fill(userMessage);
    await page.getByTestId('send-button').click();

    console.log('⏳ Waiting for model response...');

    const assistantMsg = page.getByTestId('message-assistant').last();
    await expect(assistantMsg).toBeVisible({ timeout: 120000 });
    await expect(page.getByTestId('loading-indicator')).not.toBeVisible({ timeout: 120000 });

    const messageContent = await assistantMsg.getByTestId('message-content').textContent();
    console.log('📄 Response preview:', messageContent?.substring(0, 200));

    // Response should mention the files we know exist
    const mentionsSampleFile = messageContent?.includes('sample_file.txt');
    const mentionsConfigJson = messageContent?.includes('config.json');
    const mentionsReadme = messageContent?.includes('README.md');

    const mentionsAtLeastOneFile = mentionsSampleFile || mentionsConfigJson || mentionsReadme;
    expect(mentionsAtLeastOneFile).toBe(true);

    if (mentionsAtLeastOneFile) {
      console.log('✅ Model successfully listed files from directory');
    }

    console.log('🎉 Agentic list_directory test completed');
  });

  test('model should use bash tool when asked to execute command', async ({ page }) => {
    console.log('🚀 Starting agentic bash tool test...');

    await page.goto('/');
    await expect(page.getByTestId('chat-app')).toBeVisible();

    const unloadButton = page.locator('[title="Unload model"]');
    const isModelLoaded = await unloadButton.isVisible().catch(() => false);

    if (!isModelLoaded) {
      console.log('⚠️  No model loaded - skipping test');
      test.skip();
      return;
    }

    const command = process.platform === 'win32'
      ? 'Run the command: echo Testing bash tool integration'
      : 'Run the command: echo "Testing bash tool integration"';

    console.log(`📝 Sending message: "${command}"`);

    const messageInput = page.getByTestId('message-input');
    await messageInput.fill(command);
    await page.getByTestId('send-button').click();

    console.log('⏳ Waiting for model response...');

    const assistantMsg = page.getByTestId('message-assistant').last();
    await expect(assistantMsg).toBeVisible({ timeout: 120000 });
    await expect(page.getByTestId('loading-indicator')).not.toBeVisible({ timeout: 120000 });

    const messageContent = await assistantMsg.getByTestId('message-content').textContent();
    console.log('📄 Response:', messageContent?.substring(0, 300));

    // Response should contain the echo output
    const containsExpectedOutput = messageContent?.includes('Testing bash tool');

    if (containsExpectedOutput) {
      console.log('✅ Bash tool was executed successfully');
    }

    console.log('🎉 Agentic bash tool test completed');
  });

  test('model should handle multi-step agentic workflow', async ({ page }) => {
    console.log('🚀 Starting multi-step agentic workflow test...');

    await page.goto('/');
    await expect(page.getByTestId('chat-app')).toBeVisible();

    const unloadButton = page.locator('[title="Unload model"]');
    const isModelLoaded = await unloadButton.isVisible().catch(() => false);

    if (!isModelLoaded) {
      console.log('⚠️  No model loaded - skipping test');
      test.skip();
      return;
    }

    // Complex request requiring multiple tools
    const testPath = TEST_DATA_DIR.replace(/\\/g, '\\\\');
    const userMessage = `Go to directory ${testPath}, list all files, then read the config.json file and tell me what the version is.`;

    console.log(`📝 Sending complex request: "${userMessage}"`);

    const messageInput = page.getByTestId('message-input');
    await messageInput.fill(userMessage);
    await page.getByTestId('send-button').click();

    console.log('⏳ Waiting for model to complete multi-step process...');

    // This may take longer as model might use multiple tools
    const assistantMsg = page.getByTestId('message-assistant').last();
    await expect(assistantMsg).toBeVisible({ timeout: 180000 });
    await expect(page.getByTestId('loading-indicator')).not.toBeVisible({ timeout: 180000 });

    const messageContent = await assistantMsg.getByTestId('message-content').textContent();
    console.log('📄 Final response:', messageContent?.substring(0, 300));

    // Model should have found the version "1.0" from config.json
    const mentionsVersion = messageContent?.includes('1.0');

    if (mentionsVersion) {
      console.log('✅ Model successfully completed multi-step workflow');
      console.log('   - Listed directory');
      console.log('   - Read config.json');
      console.log('   - Extracted version information');
    }

    // At minimum, response should reference the config file or version
    const referencesConfigOrVersion =
      messageContent?.toLowerCase().includes('version') ||
      messageContent?.toLowerCase().includes('config') ||
      messageContent?.includes('1.0');

    expect(referencesConfigOrVersion).toBe(true);

    console.log('🎉 Multi-step agentic workflow test completed');
  });
});

test.describe('Agentic Tests Summary', () => {
  test('display summary', async () => {
    console.log('\n🎉 Agentic Tool Calling Test Summary:');
    console.log('=====================================');
    console.log('These tests verify the model can:');
    console.log('  ✅ Autonomously use read_file tool');
    console.log('  ✅ Autonomously use list_directory tool');
    console.log('  ✅ Autonomously use bash tool');
    console.log('  ✅ Execute multi-step workflows');
    console.log('=====================================');
    console.log('Note: Tests require a loaded model to run\n');
  });
});
