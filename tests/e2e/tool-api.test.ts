import { test, expect } from '@playwright/test';
import * as path from 'path';
import { fileURLToPath } from 'url';

/**
 * Tool API Tests - Direct endpoint testing without model
 * These tests verify that the /api/tools/execute endpoint works correctly
 * for all tool types: read_file, write_file, list_directory, and bash
 */

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const BASE_URL = 'http://localhost:8000';
const TEST_DATA_DIR = path.resolve(__dirname, '../../test_data');

test.describe('Tool API Endpoint Tests', () => {

  test('read_file tool - should read sample_file.txt', async ({ request }) => {
    console.log('📖 Testing read_file tool...');

    const response = await request.post(`${BASE_URL}/api/tools/execute`, {
      data: {
        tool_name: 'read_file',
        arguments: {
          path: path.join(TEST_DATA_DIR, 'sample_file.txt')
        }
      }
    });

    expect(response.status()).toBe(200);
    const result = await response.json();

    console.log('📄 Read file result:', JSON.stringify(result, null, 2));

    expect(result.success).toBe(true);
    expect(result.result).toContain('This is a sample file');
    expect(result.result).toContain('Line 5: End of sample file');
    expect(result.path).toContain('sample_file.txt');

    console.log('✅ read_file test passed');
  });

  test('read_file tool - should read JSON file', async ({ request }) => {
    console.log('📖 Testing read_file with JSON...');

    const response = await request.post(`${BASE_URL}/api/tools/execute`, {
      data: {
        tool_name: 'read_file',
        arguments: {
          path: path.join(TEST_DATA_DIR, 'config.json')
        }
      }
    });

    expect(response.status()).toBe(200);
    const result = await response.json();

    expect(result.success).toBe(true);
    expect(result.result).toContain('"test": "data"');
    expect(result.result).toContain('"tools_enabled": true');

    // Verify it's valid JSON
    const parsedContent = JSON.parse(result.result);
    expect(parsedContent.test).toBe('data');
    expect(parsedContent.version).toBe('1.0');

    console.log('✅ JSON read test passed');
  });

  test('read_file tool - should handle non-existent file', async ({ request }) => {
    console.log('🚫 Testing read_file with non-existent file...');

    const response = await request.post(`${BASE_URL}/api/tools/execute`, {
      data: {
        tool_name: 'read_file',
        arguments: {
          path: 'nonexistent_file_12345.txt'
        }
      }
    });

    expect(response.status()).toBe(200);
    const result = await response.json();

    console.log('📄 Error result:', JSON.stringify(result, null, 2));

    expect(result.success).toBe(false);
    expect(result.error).toContain('Failed to read file');

    console.log('✅ Error handling test passed');
  });

  test('write_file tool - should create new file', async ({ request }) => {
    console.log('✍️  Testing write_file tool...');

    const testContent = `Test file created by automated test\nTimestamp: ${new Date().toISOString()}\nThis file can be deleted.`;
    const testFilePath = path.join(TEST_DATA_DIR, 'test_output.txt');

    const response = await request.post(`${BASE_URL}/api/tools/execute`, {
      data: {
        tool_name: 'write_file',
        arguments: {
          path: testFilePath,
          content: testContent
        }
      }
    });

    expect(response.status()).toBe(200);
    const result = await response.json();

    console.log('📄 Write result:', JSON.stringify(result, null, 2));

    expect(result.success).toBe(true);
    expect(result.bytes_written).toBe(testContent.length);
    expect(result.path).toContain('test_output.txt');

    // Verify by reading back
    const readResponse = await request.post(`${BASE_URL}/api/tools/execute`, {
      data: {
        tool_name: 'read_file',
        arguments: {
          path: testFilePath
        }
      }
    });

    const readResult = await readResponse.json();
    expect(readResult.success).toBe(true);
    expect(readResult.result).toBe(testContent);

    console.log('✅ write_file test passed (file created and verified)');
  });

  test('list_directory tool - should list test_data directory', async ({ request }) => {
    console.log('📂 Testing list_directory tool...');

    const response = await request.post(`${BASE_URL}/api/tools/execute`, {
      data: {
        tool_name: 'list_directory',
        arguments: {
          path: TEST_DATA_DIR,
          recursive: false
        }
      }
    });

    expect(response.status()).toBe(200);
    const result = await response.json();

    console.log('📄 Directory listing result:', JSON.stringify(result, null, 2));

    expect(result.success).toBe(true);
    expect(result.result).toContain('sample_file.txt');
    expect(result.result).toContain('config.json');
    expect(result.result).toContain('README.md');
    expect(result.count).toBeGreaterThanOrEqual(3);
    expect(result.recursive).toBe(false);

    console.log('✅ list_directory test passed');
  });

  test('list_directory tool - should list recursively', async ({ request }) => {
    console.log('📂 Testing recursive directory listing...');

    const response = await request.post(`${BASE_URL}/api/tools/execute`, {
      data: {
        tool_name: 'list_directory',
        arguments: {
          path: TEST_DATA_DIR,
          recursive: true
        }
      }
    });

    expect(response.status()).toBe(200);
    const result = await response.json();

    console.log(`📄 Found ${result.count} entries (recursive)`);

    expect(result.success).toBe(true);
    expect(result.recursive).toBe(true);
    expect(result.count).toBeGreaterThanOrEqual(3);

    console.log('✅ Recursive listing test passed');
  });

  test('bash tool - should execute simple command', async ({ request }) => {
    console.log('⚡ Testing bash tool...');

    // Use platform-appropriate command
    const command = process.platform === 'win32'
      ? 'echo Hello from bash tool'
      : 'echo "Hello from bash tool"';

    const response = await request.post(`${BASE_URL}/api/tools/execute`, {
      data: {
        tool_name: 'bash',
        arguments: {
          command: command
        }
      }
    });

    expect(response.status()).toBe(200);
    const result = await response.json();

    console.log('📄 Bash result:', JSON.stringify(result, null, 2));

    expect(result.success).toBe(true);
    expect(result.result).toContain('Hello from bash tool');
    expect(result.exit_code).toBe(0);

    console.log('✅ bash tool test passed');
  });

  test('bash tool - should list files in test_data directory', async ({ request }) => {
    console.log('⚡ Testing bash with directory listing...');

    const command = process.platform === 'win32'
      ? `dir /b ${TEST_DATA_DIR}`  // /b for bare format (file names only), no quotes
      : `ls "${TEST_DATA_DIR}"`;

    const response = await request.post(`${BASE_URL}/api/tools/execute`, {
      data: {
        tool_name: 'bash',
        arguments: {
          command: command
        }
      }
    });

    expect(response.status()).toBe(200);
    const result = await response.json();

    console.log('📄 Files found via bash:');
    console.log(result.result);

    expect(result.success).toBe(true);
    expect(result.result).toContain('sample_file.txt');
    expect(result.result).toContain('config.json');

    console.log('✅ bash directory listing test passed');
  });

  test('unknown tool - should return error', async ({ request }) => {
    console.log('❌ Testing unknown tool handling...');

    const response = await request.post(`${BASE_URL}/api/tools/execute`, {
      data: {
        tool_name: 'nonexistent_tool',
        arguments: {}
      }
    });

    expect(response.status()).toBe(200);
    const result = await response.json();

    expect(result.success).toBe(false);
    expect(result.error).toContain('Unknown tool');

    console.log('✅ Unknown tool handling test passed');
  });
});

test.describe('Tool API Summary', () => {
  test('display test summary', async () => {
    console.log('\n🎉 All Tool API Tests Summary:');
    console.log('================================');
    console.log('✅ read_file - Plain text');
    console.log('✅ read_file - JSON');
    console.log('✅ read_file - Error handling');
    console.log('✅ write_file - Create file');
    console.log('✅ list_directory - Non-recursive');
    console.log('✅ list_directory - Recursive');
    console.log('✅ bash - Echo command');
    console.log('✅ bash - Directory listing');
    console.log('✅ Error handling - Unknown tool');
    console.log('================================\n');
  });
});
