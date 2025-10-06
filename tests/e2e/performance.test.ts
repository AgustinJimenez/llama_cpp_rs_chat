import { test, expect } from '@playwright/test';

test.describe('Performance Tests', () => {
  test('page loads within acceptable time', async ({ page }) => {
    const startTime = Date.now();
    
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    
    const loadTime = Date.now() - startTime;
    
    // Page should load within 5 seconds
    expect(loadTime).toBeLessThan(5000);
    
    console.log(`Page load time: ${loadTime}ms`);
  });

  test('static assets load efficiently', async ({ page }) => {
    const responses: any[] = [];
    
    page.on('response', response => {
      if (response.url().includes('/assets/') || response.url().includes('.js') || response.url().includes('.css')) {
        responses.push({
          url: response.url(),
          status: response.status(),
          size: response.headers()['content-length']
        });
      }
    });
    
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    
    // All assets should load successfully
    for (const response of responses) {
      expect(response.status).toBe(200);
    }
    
    console.log(`Loaded ${responses.length} static assets`);
  });

  test('API response time is acceptable', async ({ request }) => {
    const startTime = Date.now();
    
    const response = await request.post('/api/chat', {
      headers: {
        'Content-Type': 'application/json',
      },
      data: {
        message: 'Hello, quick test message',
      }
    });
    
    const responseTime = Date.now() - startTime;
    
    expect(response.status()).toBe(200);
    
    // API should respond within 30 seconds (LLaMA can be slow)
    expect(responseTime).toBeLessThan(30000);
    
    console.log(`API response time: ${responseTime}ms`);
  });

  test('memory usage remains stable during chat session', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    
    // Get initial memory usage
    const initialMemory = await page.evaluate(() => {
      return (performance as any).memory?.usedJSHeapSize || 0;
    });
    
    const chatInput = page.locator('input[placeholder*="message"], textarea[placeholder*="message"], input[type="text"]').first();
    const sendButton = page.locator('button').filter({ hasText: /send|submit|→|➤/i }).first();
    
    // Send multiple messages to test memory stability
    for (let i = 0; i < 3; i++) {
      await chatInput.fill(`Test message ${i + 1}`);
      
      if (await sendButton.count() > 0) {
        await sendButton.click();
      } else {
        await chatInput.press('Enter');
      }
      
      // Wait between messages
      await page.waitForTimeout(3000);
    }
    
    // Check final memory usage
    const finalMemory = await page.evaluate(() => {
      return (performance as any).memory?.usedJSHeapSize || 0;
    });
    
    if (initialMemory > 0 && finalMemory > 0) {
      const memoryIncrease = finalMemory - initialMemory;
      
      // Memory shouldn't increase dramatically (less than 50MB)
      expect(memoryIncrease).toBeLessThan(50 * 1024 * 1024);
      
      console.log(`Memory increase: ${(memoryIncrease / 1024 / 1024).toFixed(2)}MB`);
    }
  });

  test('handles concurrent API requests efficiently', async ({ request }) => {
    const concurrentRequests = 3;
    const promises = [];
    
    const startTime = Date.now();
    
    for (let i = 0; i < concurrentRequests; i++) {
      promises.push(
        request.post('/api/chat', {
          headers: {
            'Content-Type': 'application/json',
          },
          data: {
            message: `Concurrent test message ${i + 1}`,
          }
        })
      );
    }
    
    const responses = await Promise.all(promises);
    const totalTime = Date.now() - startTime;
    
    // All requests should succeed
    for (const response of responses) {
      expect(response.status()).toBe(200);
    }
    
    // Concurrent requests shouldn't take too much longer than single request
    // Allow up to 60 seconds for concurrent LLaMA processing
    expect(totalTime).toBeLessThan(60000);
    
    console.log(`${concurrentRequests} concurrent requests completed in ${totalTime}ms`);
  });

  test('UI remains responsive during API calls', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    
    const chatInput = page.locator('input[placeholder*="message"], textarea[placeholder*="message"], input[type="text"]').first();
    
    // Start a chat request
    await chatInput.fill('This is a test message for responsiveness');
    
    const sendButton = page.locator('button').filter({ hasText: /send|submit|→|➤/i }).first();
    if (await sendButton.count() > 0) {
      await sendButton.click();
    } else {
      await chatInput.press('Enter');
    }
    
    // Immediately try to interact with UI while request is processing
    await page.waitForTimeout(1000); // Wait 1 second
    
    // UI should still be responsive - try typing in input
    await chatInput.fill('Another message while first is processing');
    
    // Input should accept the new text
    await expect(chatInput).toHaveValue('Another message while first is processing');
    
    // Clear the input
    await chatInput.clear();
  });
});