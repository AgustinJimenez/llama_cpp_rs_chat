import { test, expect } from '@playwright/test';

test.describe('Accessibility Tests', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
  });

  test('page has proper semantic structure', async ({ page }) => {
    // Check for main landmark
    const main = page.locator('main, [role="main"]');
    if (await main.count() > 0) {
      await expect(main.first()).toBeVisible();
    }
    
    // Check for proper heading hierarchy
    const h1 = page.locator('h1');
    await expect(h1).toHaveCount(1); // Should have exactly one h1
    
    // Check that chat interface has proper labels
    const chatInput = page.locator('input[placeholder*="message"], textarea[placeholder*="message"], input[type="text"]').first();
    
    // Input should have accessible name (aria-label, label, or placeholder)
    const hasAccessibleName = await chatInput.evaluate(input => {
      return !!(
        input.getAttribute('aria-label') ||
        input.getAttribute('aria-labelledby') ||
        input.getAttribute('placeholder') ||
        input.closest('label')
      );
    });
    
    expect(hasAccessibleName).toBe(true);
  });

  test('supports keyboard navigation', async ({ page }) => {
    const chatInput = page.locator('input[placeholder*="message"], textarea[placeholder*="message"], input[type="text"]').first();
    
    // Tab to the input field
    await page.keyboard.press('Tab');
    
    // Check if input is focused
    const isFocused = await chatInput.evaluate(input => document.activeElement === input);
    expect(isFocused).toBe(true);
    
    // Type a message
    await page.keyboard.type('Hello, testing keyboard navigation');
    
    // Try to submit with Enter key
    await page.keyboard.press('Enter');
    
    // Message should be sent (input should clear or message should appear)
    await page.waitForTimeout(2000);
    
    // Check if message was sent by looking for the text
    const messageVisible = await page.locator('text=Hello, testing keyboard navigation').count() > 0;
    expect(messageVisible).toBe(true);
  });

  test('has proper focus management', async ({ page }) => {
    const chatInput = page.locator('input[placeholder*="message"], textarea[placeholder*="message"], input[type="text"]').first();
    
    // Click the input to focus it
    await chatInput.click();
    
    // Check if input has focus
    let isFocused = await chatInput.evaluate(input => document.activeElement === input);
    expect(isFocused).toBe(true);
    
    // Send a message
    await chatInput.fill('Focus management test');
    await chatInput.press('Enter');
    
    // After sending, input should remain focused or regain focus
    await page.waitForTimeout(1000);
    isFocused = await chatInput.evaluate(input => document.activeElement === input);
    expect(isFocused).toBe(true);
  });

  test('has sufficient color contrast', async ({ page }) => {
    // Get all text elements
    const textElements = await page.locator('*').evaluateAll(elements => {
      return elements
        .filter(el => {
          const style = window.getComputedStyle(el);
          return style.color && style.backgroundColor && el.textContent?.trim();
        })
        .map(el => {
          const style = window.getComputedStyle(el);
          return {
            color: style.color,
            backgroundColor: style.backgroundColor,
            fontSize: style.fontSize,
            text: el.textContent?.trim()
          };
        });
    });
    
    // Basic contrast check (simplified)
    for (const element of textElements.slice(0, 10)) { // Check first 10 elements
      if (element.text && element.text.length > 0) {
        // Color values should not be the same (indicating proper contrast)
        expect(element.color).not.toBe(element.backgroundColor);
      }
    }
  });

  test('supports screen readers with proper ARIA labels', async ({ page }) => {
    // Check for ARIA landmarks
    const landmarks = await page.locator('[role="main"], [role="navigation"], [role="banner"], [role="contentinfo"]').count();
    
    // Should have at least main content area
    expect(landmarks).toBeGreaterThan(0);
    
    // Check chat input has proper ARIA attributes
    const chatInput = page.locator('input[placeholder*="message"], textarea[placeholder*="message"], input[type="text"]').first();
    
    const ariaLabel = await chatInput.getAttribute('aria-label');
    const ariaLabelledBy = await chatInput.getAttribute('aria-labelledby');
    const placeholder = await chatInput.getAttribute('placeholder');
    
    // Should have some form of accessible name
    expect(ariaLabel || ariaLabelledBy || placeholder).toBeTruthy();
    
    // Check for any dynamic content that should be announced
    const liveRegions = await page.locator('[aria-live], [role="status"], [role="alert"]').count();
    
    // Chat applications should have live regions for new messages
    // This might be 0 if not implemented yet, but documents the requirement
    console.log(`Found ${liveRegions} live regions for screen reader announcements`);
  });

  test('handles high contrast mode', async ({ page }) => {
    // Simulate high contrast mode by injecting CSS
    await page.addStyleTag({
      content: `
        *, *::before, *::after {
          background-color: black !important;
          color: white !important;
          border-color: white !important;
        }
        input, textarea, button {
          background-color: black !important;
          color: white !important;
          border: 2px solid white !important;
        }
      `
    });
    
    await page.waitForTimeout(1000);
    
    // Check that interface is still usable
    const chatInput = page.locator('input[placeholder*="message"], textarea[placeholder*="message"], input[type="text"]').first();
    await expect(chatInput).toBeVisible();
    
    // Try to interact with the interface
    await chatInput.fill('High contrast test');
    
    const sendButton = page.locator('button').filter({ hasText: /send|submit|→|➤/i }).first();
    if (await sendButton.count() > 0) {
      await expect(sendButton).toBeVisible();
    }
  });

  test('supports zoom up to 200%', async ({ page }) => {
    // Set zoom to 200%
    await page.setViewportSize({ width: 640, height: 480 }); // Simulate 200% zoom by halving viewport
    
    await page.reload();
    await page.waitForLoadState('networkidle');
    
    // Interface should still be usable
    const chatInput = page.locator('input[placeholder*="message"], textarea[placeholder*="message"], input[type="text"]').first();
    await expect(chatInput).toBeVisible();
    
    // Check that text is not cut off
    const heading = page.locator('h1').first();
    if (await heading.count() > 0) {
      const boundingBox = await heading.boundingBox();
      expect(boundingBox?.width).toBeGreaterThan(0);
      expect(boundingBox?.height).toBeGreaterThan(0);
    }
    
    // Should be able to send a message
    await chatInput.fill('Zoom test message');
    await chatInput.press('Enter');
    
    // Message should appear
    await expect(page.locator('text=Zoom test message')).toBeVisible({ timeout: 10000 });
  });

  test('provides alternative text for images', async ({ page }) => {
    // Check all images have alt text
    const images = page.locator('img');
    const imageCount = await images.count();
    
    if (imageCount > 0) {
      for (let i = 0; i < imageCount; i++) {
        const img = images.nth(i);
        const alt = await img.getAttribute('alt');
        const role = await img.getAttribute('role');
        
        // Images should have alt text or be marked as decorative
        expect(alt !== null || role === 'presentation').toBe(true);
      }
    }
  });

  test('error messages are accessible', async ({ page }) => {
    // Trigger an error by sending malformed request
    await page.route('/api/chat', route => {
      route.fulfill({
        status: 500,
        body: JSON.stringify({ error: 'Server error' })
      });
    });
    
    const chatInput = page.locator('input[placeholder*="message"], textarea[placeholder*="message"], input[type="text"]').first();
    await chatInput.fill('This should trigger an error');
    await chatInput.press('Enter');
    
    // Wait for error message
    await page.waitForTimeout(3000);
    
    // Look for error indication
    const errorElements = page.locator('[role="alert"], .error, text=/error|failed/i');
    
    if (await errorElements.count() > 0) {
      // Error should be visible and have proper ARIA attributes
      const firstError = errorElements.first();
      await expect(firstError).toBeVisible();
      
      // Check if it has proper ARIA role
      const role = await firstError.getAttribute('role');
      const ariaLive = await firstError.getAttribute('aria-live');
      
      // Should have alert role or live region for screen readers
      expect(role === 'alert' || ariaLive === 'polite' || ariaLive === 'assertive').toBe(true);
    }
  });
});