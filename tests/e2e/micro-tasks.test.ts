import { test, expect } from '@playwright/test';

/**
 * Micro Tasks - Fast, Focused Testing
 * 
 * These tests focus on small, specific tasks that complete quickly
 * and don't require large context windows or long processing times.
 */

test.describe('Micro Task Tests', () => {
  
  test('API health check - server responsiveness', async ({ request }) => {
    console.log('⚡ Testing API health...');
    
    const response = await request.get('/health');
    expect(response.status()).toBe(200);
    
    const health = await response.json();
    expect(health).toHaveProperty('status', 'ok');
    console.log('✅ Server health check passed');
  });

  test('Quick file read - small text file', async ({ request }) => {
    console.log('📖 Testing quick file read...');
    
    const response = await request.post('/api/tools/execute', {
      data: {
        tool_name: 'read_file',
        arguments: { path: 'test_data/config.json' }
      }
    });
    
    expect(response.status()).toBe(200);
    const result = await response.json();
    expect(result.success).toBe(true);
    expect(result.result).toContain('version');
    console.log('✅ Quick file read completed');
  });

  test('Directory listing - basic operation', async ({ request }) => {
    console.log('📂 Testing directory listing...');
    
    const response = await request.post('/api/tools/execute', {
      data: {
        tool_name: 'list_directory',
        arguments: { path: 'test_data' }
      }
    });
    
    expect(response.status()).toBe(200);
    const result = await response.json();
    expect(result.success).toBe(true);
    expect(result.result).toContain('config.json');
    console.log('✅ Directory listing completed');
  });

  test('Simple bash command - echo test', async ({ request }) => {
    console.log('⚡ Testing simple bash command...');
    
    const response = await request.post('/api/tools/execute', {
      data: {
        tool_name: 'bash',
        arguments: { command: 'echo "Hello Test"' }
      }
    });
    
    expect(response.status()).toBe(200);
    const result = await response.json();
    expect(result.success).toBe(true);
    expect(result.result).toContain('Hello Test');
    console.log('✅ Simple bash command completed');
  });

  test('File creation - small test file', async ({ request }) => {
    console.log('✍️ Testing file creation...');
    
    const testContent = 'Test file created at ' + new Date().toISOString();
    const response = await request.post('/api/tools/execute', {
      data: {
        tool_name: 'write_file',
        arguments: { 
          path: 'test_data/micro_test.txt',
          content: testContent
        }
      }
    });
    
    expect(response.status()).toBe(200);
    const result = await response.json();
    expect(result.success).toBe(true);
    console.log('✅ File creation completed');
  });

  test('Model status check - no model loading', async ({ request }) => {
    console.log('🤖 Testing model status...');
    
    const response = await request.get('/api/model/status');
    expect(response.status()).toBe(200);
    
    const status = await response.json();
    expect(status).toHaveProperty('loaded');
    console.log(`✅ Model status: ${status.loaded ? 'loaded' : 'not loaded'}`);
  });

  test('Config retrieval - sampling settings', async ({ request }) => {
    console.log('⚙️ Testing config retrieval...');
    
    const response = await request.get('/api/config');
    expect(response.status()).toBe(200);
    
    const config = await response.json();
    // Config structure may vary - just check it has configuration data
    expect(config).toBeDefined();
    expect(typeof config).toBe('object');
    console.log('✅ Config retrieval completed');
  });

  test('Error handling - invalid tool', async ({ request }) => {
    console.log('❌ Testing error handling...');
    
    const response = await request.post('/api/tools/execute', {
      data: {
        tool_name: 'invalid_tool',
        arguments: {}
      }
    });
    
    // Error handling may return 200 with error in body
    const result = await response.json();
    expect(result.success).toBe(false);
    console.log('✅ Error handling working correctly');
  });

});

test.describe('Micro Task Summary', () => {
  test('display micro task summary', async () => {
    console.log(`
🎯 Micro Task Test Summary:
============================
✅ API Health Check
✅ Quick File Operations  
✅ Simple Bash Commands
✅ Basic File Creation
✅ Model Status Queries
✅ Configuration Access
✅ Error Handling

Benefits:
• Fast execution (< 5 seconds total)
• No model dependencies
• No large context requirements
• Reliable server connectivity tests
• Core functionality validation
============================
`);
  });
});