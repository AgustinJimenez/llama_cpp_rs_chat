import { test, expect } from '@playwright/test';

test.describe('Debug Chat Interface', () => {
  test('debug the chat interface elements', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    
    // Take a screenshot to see what we're working with
    await page.screenshot({ path: 'debug-initial.png', fullPage: true });
    
    // Debug: print all input elements
    const inputs = await page.locator('input, textarea, button').all();
    console.log(`Found ${inputs.length} interactive elements`);
    
    for (let i = 0; i < inputs.length; i++) {
      const element = inputs[i];
      const tagName = await element.evaluate(el => el.tagName);
      const type = await element.getAttribute('type');
      const placeholder = await element.getAttribute('placeholder');
      const text = await element.textContent();
      
      console.log(`Element ${i}: ${tagName} type=${type} placeholder="${placeholder}" text="${text}"`);
    }
    
    // Try to find the main chat input
    const chatInput = page.locator('input').first();
    if (await chatInput.count() > 0) {
      await chatInput.fill('Hello debug test!');
      await page.screenshot({ path: 'debug-typed.png', fullPage: true });
      
      // Try Enter key
      await chatInput.press('Enter');
      await page.waitForTimeout(3000);
      await page.screenshot({ path: 'debug-after-enter.png', fullPage: true });
      
      // Print page content to see what happened
      const bodyText = await page.textContent('body');
      console.log('Page content after Enter:', bodyText);
    }
  });
});