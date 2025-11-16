import { test, expect } from '@playwright/test';

test.describe('Long HTML Response Generation', () => {
  test('should generate complete HTML without stopping at </script> tags', async ({ page }) => {
    // Navigate to the app
    await page.goto('http://localhost:8000');

    // Wait for the app to load
    await page.waitForSelector('[data-testid="model-selector"]', { timeout: 10000 });

    // Load model
    const modelPath = 'E:\\.lmstudio\\models\\lmstudio-community\\Devstral-Small-2507-GGUF\\Devstral-Small-2507-Q4_K_M.gguf';

    // Open model selector
    await page.click('[data-testid="model-selector"]');

    // Enter model path
    await page.fill('input[placeholder*="model path"]', modelPath);

    // Click load button
    await page.click('button:has-text("Load Model")');

    // Wait for model to load (this can take a while)
    await page.waitForSelector('text=/Model loaded|Loaded/', { timeout: 60000 });

    // Type a message asking for a complete HTML page with script tags
    const messageInput = page.locator('textarea[placeholder*="message" i], textarea[placeholder*="type" i]').first();
    await messageInput.fill('Write me a complete HTML login page with inline JavaScript and CSS. Include form validation in the <script> tags.');

    // Send the message
    await page.keyboard.press('Enter');

    // Wait for response to start
    await page.waitForSelector('.message-bubble:last-child', { timeout: 10000 });

    // Wait for generation to complete (look for stop indicator or timeout)
    // We'll wait up to 2 minutes for generation
    await page.waitForTimeout(120000);

    // Get the last response
    const lastMessage = await page.locator('.message-bubble').last().textContent();

    console.log('Response length:', lastMessage?.length);
    console.log('Response preview:', lastMessage?.substring(0, 500));

    // Assertions
    expect(lastMessage).toBeTruthy();
    expect(lastMessage!.length).toBeGreaterThan(500); // Should be a substantial response

    // Check that it contains HTML structure
    expect(lastMessage).toContain('<html');
    expect(lastMessage).toContain('</html>');

    // Most importantly: check that </script> tags are complete
    expect(lastMessage).toContain('</script>');

    // Verify it didn't cut off mid-tag
    const scriptClosingTags = lastMessage!.match(/<\/script>/g);
    expect(scriptClosingTags).toBeTruthy();
    expect(scriptClosingTags!.length).toBeGreaterThan(0);

    // Check that we don't have incomplete </s without the rest of "cript>"
    // This would indicate the bug is still present
    const incompleteScriptTag = lastMessage!.includes('</s\n') ||
                                 lastMessage!.endsWith('</s') ||
                                 lastMessage!.includes('</sc\n');
    expect(incompleteScriptTag).toBe(false);

    console.log('✅ Test passed: HTML generation completed without premature stopping');
  });
});
