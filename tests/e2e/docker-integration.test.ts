import { test, expect } from '@playwright/test';

test.describe('Docker Integration Tests', () => {
  test('container health check passes', async ({ request }) => {
    const response = await request.get('/health');
    expect(response.status()).toBe(200);
    
    const data = await response.json();
    expect(data.status).toBe('ok');
    expect(data.service).toBe('llama-chat-web');
  });

  test('static files are served correctly from container', async ({ page }) => {
    await page.goto('/');
    
    // Check that CSS is loaded
    const styles = await page.evaluate(() => {
      const styleSheets = Array.from(document.styleSheets);
      return styleSheets.length > 0;
    });
    expect(styles).toBe(true);
    
    // Check that JavaScript is loaded and executed
    const hasReact = await page.evaluate(() => {
      return typeof window.React !== 'undefined' || document.querySelector('#root')?.hasChildNodes();
    });
    expect(hasReact).toBe(true);
  });

  test('LLaMA model integration works in container', async ({ request }) => {
    // Test that the containerized LLaMA model responds
    const response = await request.post('/api/chat', {
      headers: {
        'Content-Type': 'application/json',
      },
      data: {
        message: 'What is Docker?',
      }
    });

    expect(response.status()).toBe(200);
    
    const data = await response.json();
    expect(data.message.content).toBeTruthy();
    expect(data.message.content.length).toBeGreaterThan(10);
    
    // Response should be relevant to Docker (basic content check)
    const content = data.message.content.toLowerCase();
    const dockerRelated = content.includes('docker') || 
                          content.includes('container') || 
                          content.includes('software') ||
                          content.includes('application');
    
    expect(dockerRelated).toBe(true);
  });

  test('model files are accessible in container', async ({ request }) => {
    // Test that LLaMA can generate responses (indicating model is loaded)
    const testMessages = [
      'Hello',
      'What is 1+1?',
      'Tell me about AI'
    ];
    
    for (const message of testMessages) {
      const response = await request.post('/api/chat', {
        headers: {
          'Content-Type': 'application/json',
        },
        data: { message }
      });
      
      expect(response.status()).toBe(200);
      
      const data = await response.json();
      expect(data.message.content).toBeTruthy();
      expect(data.message.content.length).toBeGreaterThan(0);
      
      // Should not be error messages
      expect(data.message.content).not.toContain('Failed to load model');
      expect(data.message.content).not.toContain('Model not found');
    }
  });

  test('container handles concurrent requests', async ({ request }) => {
    const promises = [];
    
    // Send 3 concurrent requests
    for (let i = 0; i < 3; i++) {
      promises.push(
        request.post('/api/chat', {
          headers: {
            'Content-Type': 'application/json',
          },
          data: {
            message: `Concurrent test ${i + 1}`,
          }
        })
      );
    }
    
    const responses = await Promise.all(promises);
    
    // All should succeed
    for (const response of responses) {
      expect(response.status()).toBe(200);
      
      const data = await response.json();
      expect(data.message.content).toBeTruthy();
    }
  });

  test('container memory and performance', async ({ request }) => {
    // Send a complex request that might use more resources
    const complexMessage = 'Please write a detailed explanation about machine learning, artificial intelligence, and how neural networks work. Include examples and be comprehensive.';
    
    const startTime = Date.now();
    const response = await request.post('/api/chat', {
      headers: {
        'Content-Type': 'application/json',
      },
      data: {
        message: complexMessage,
      }
    });
    const responseTime = Date.now() - startTime;
    
    expect(response.status()).toBe(200);
    
    const data = await response.json();
    expect(data.message.content).toBeTruthy();
    expect(data.message.content.length).toBeGreaterThan(50);
    
    // Should complete within reasonable time (2 minutes for complex request)
    expect(responseTime).toBeLessThan(120000);
    
    console.log(`Complex request completed in ${responseTime}ms`);
  });

  test('container CORS configuration', async ({ request }) => {
    // Test CORS headers for cross-origin requests
    const response = await request.post('/api/chat', {
      headers: {
        'Content-Type': 'application/json',
        'Origin': 'http://external-domain.com'
      },
      data: {
        message: 'CORS test',
      }
    });
    
    expect(response.status()).toBe(200);
    
    // Check CORS headers
    expect(response.headers()['access-control-allow-origin']).toBe('*');
  });

  test('container environment configuration', async ({ request }) => {
    // Test that the configuration endpoint returns expected values
    const response = await request.get('/api/config');
    expect(response.status()).toBe(200);
    
    const config = await response.json();
    
    // Should have expected configuration structure for Docker environment
    expect(config).toMatchObject({
      sampler_type: 'Greedy',
      temperature: 0.7,
      top_p: 0.95,
      top_k: 20,
      mirostat_tau: 5.0,
      mirostat_eta: 0.1
    });
  });

  test('container error handling', async ({ request }) => {
    // Test that container handles malformed requests gracefully
    const response = await request.post('/api/chat', {
      headers: {
        'Content-Type': 'application/json',
      },
      data: 'invalid json'
    });
    
    expect(response.status()).toBe(400);
    
    const error = await response.json();
    expect(error.error).toContain('Invalid JSON format');
  });

  test('container asset serving', async ({ page, request }) => {
    await page.goto('/');
    
    // Get all asset URLs
    const assetUrls = await page.evaluate(() => {
      const assets = [];
      
      // Get CSS files
      const links = Array.from(document.querySelectorAll('link[rel="stylesheet"]'));
      assets.push(...links.map(link => (link as HTMLLinkElement).href));
      
      // Get JS files
      const scripts = Array.from(document.querySelectorAll('script[src]'));
      assets.push(...scripts.map(script => (script as HTMLScriptElement).src));
      
      return assets;
    });
    
    // Test that all assets load successfully
    for (const assetUrl of assetUrls) {
      if (assetUrl.startsWith('http://localhost:3000')) {
        const path = new URL(assetUrl).pathname;
        const response = await request.get(path);
        expect(response.status()).toBe(200);
      }
    }
  });
});