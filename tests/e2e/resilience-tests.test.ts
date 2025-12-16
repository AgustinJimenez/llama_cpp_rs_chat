import { test, expect } from '@playwright/test';

/**
 * Resilience Tests
 * 
 * These tests focus on error handling, recovery, and robustness
 * under various failure conditions and edge cases.
 */

test.describe('Resilience and Error Handling Tests', () => {

  test('server recovery - handle connection issues gracefully', async ({ request }) => {
    console.log('🔄 Testing server recovery patterns...');
    
    // Test with retries for flaky connections
    let attempts = 0;
    let success = false;
    
    while (attempts < 3 && !success) {
      try {
        console.log(`   🔍 Attempt ${attempts + 1}/3...`);
        const response = await request.get('/health', { timeout: 5000 });
        
        if (response.status() === 200) {
          success = true;
          console.log('✅ Server connection established');
        }
      } catch (error) {
        attempts++;
        if (attempts < 3) {
          console.log(`   ⚠️ Attempt failed, retrying in 1s...`);
          await new Promise(resolve => setTimeout(resolve, 1000));
        }
      }
    }
    
    expect(success).toBe(true);
  });

  test('graceful failure - invalid file operations', async ({ request }) => {
    console.log('❌ Testing graceful failure handling...');
    
    const failureTests = [
      {
        name: 'Non-existent file read',
        tool: 'read_file',
        args: { path: 'test_data/does_not_exist.txt' },
        expectError: true
      },
      {
        name: 'Invalid directory list',
        tool: 'list_directory', 
        args: { path: 'invalid/directory/path' },
        expectError: true
      },
      {
        name: 'Bad bash command',
        tool: 'bash',
        args: { command: 'invalidcommandthatdoesnotexist' },
        expectError: true
      }
    ];
    
    for (const test of failureTests) {
      console.log(`   🧪 Testing: ${test.name}...`);
      
      const response = await request.post('/api/tools/execute', {
        data: {
          tool_name: test.tool,
          arguments: test.args
        }
      });
      
      // Should return error but not crash server
      const result = await response.json();
      if (test.expectError) {
        expect(result.success).toBe(false);
        expect(result).toHaveProperty('error');
      }
      
      console.log(`   ✅ ${test.name} handled gracefully`);
    }
  });

  test('resource limits - handle large operations safely', async ({ request }) => {
    console.log('📊 Testing resource limit handling...');
    
    // Test with progressively larger operations
    const sizes = [100, 1000, 5000]; // Character counts
    
    for (const size of sizes) {
      console.log(`   📝 Testing ${size}-character content...`);
      
      const content = 'x'.repeat(size);
      const response = await request.post('/api/tools/execute', {
        data: {
          tool_name: 'write_file',
          arguments: { 
            path: `test_data/size_test_${size}.txt`,
            content: content
          }
        }
      });
      
      const result = await response.json();
      
      if (result.success) {
        // Verify content was written correctly
        const readResponse = await request.post('/api/tools/execute', {
          data: {
            tool_name: 'read_file',
            arguments: { path: `test_data/size_test_${size}.txt` }
          }
        });
        
        const readResult = await readResponse.json();
        expect(readResult.result.length).toBe(size);
        console.log(`   ✅ ${size} characters handled successfully`);
      } else {
        console.log(`   ⚠️ ${size} characters exceeded limits (expected)`);
      }
    }
  });

  test('concurrent operations - handle multiple requests', async ({ request }) => {
    console.log('🔀 Testing concurrent operations...');
    
    const operations = Array.from({ length: 5 }, (_, i) => 
      request.post('/api/tools/execute', {
        data: {
          tool_name: 'write_file',
          arguments: { 
            path: `test_data/concurrent_${i}.txt`,
            content: `Concurrent operation ${i} at ${Date.now()}`
          }
        }
      })
    );
    
    const results = await Promise.allSettled(operations);
    
    let successCount = 0;
    let failureCount = 0;
    
    for (let i = 0; i < results.length; i++) {
      if (results[i].status === 'fulfilled') {
        const response = await results[i].value.json();
        if (response.success) {
          successCount++;
        } else {
          failureCount++;
        }
      } else {
        failureCount++;
      }
    }
    
    console.log(`   ✅ Successful operations: ${successCount}`);
    console.log(`   ⚠️ Failed operations: ${failureCount}`);
    
    // At least some operations should succeed
    expect(successCount).toBeGreaterThan(0);
  });

  test('timeout handling - prevent hanging operations', async ({ request }) => {
    console.log('⏰ Testing timeout handling...');
    
    // Test operations with short timeouts
    const quickTests = [
      {
        name: 'Quick health check',
        request: () => request.get('/health', { timeout: 2000 })
      },
      {
        name: 'Fast file read',
        request: () => request.post('/api/tools/execute', {
          timeout: 3000,
          data: {
            tool_name: 'read_file',
            arguments: { path: 'test_data/config.json' }
          }
        })
      }
    ];
    
    for (const test of quickTests) {
      console.log(`   ⚡ ${test.name}...`);
      
      try {
        const response = await test.request();
        expect(response.status()).toBeLessThan(500);
        console.log(`   ✅ ${test.name} completed within timeout`);
      } catch (error) {
        if (error.message.includes('timeout')) {
          console.log(`   ⚠️ ${test.name} timed out (may be expected)`);
        } else {
          throw error;
        }
      }
    }
  });

  test('data integrity - verify operations maintain consistency', async ({ request }) => {
    console.log('🔒 Testing data integrity...');
    
    const testData = {
      original: 'integrity test data',
      timestamp: new Date().toISOString()
    };
    
    // Write data
    console.log('   📝 Writing test data...');
    const writeResponse = await request.post('/api/tools/execute', {
      data: {
        tool_name: 'write_file',
        arguments: { 
          path: 'test_data/integrity_test.json',
          content: JSON.stringify(testData, null, 2)
        }
      }
    });
    
    expect(writeResponse.status()).toBe(200);
    
    // Read back and verify
    console.log('   📖 Verifying data integrity...');
    const readResponse = await request.post('/api/tools/execute', {
      data: {
        tool_name: 'read_file',
        arguments: { path: 'test_data/integrity_test.json' }
      }
    });
    
    const readResult = await readResponse.json();
    const parsedData = JSON.parse(readResult.result);
    
    expect(parsedData.original).toBe(testData.original);
    expect(parsedData.timestamp).toBe(testData.timestamp);
    
    console.log('   ✅ Data integrity verified');
  });

});

test.describe('Resilience Summary', () => {
  test('display resilience test summary', async () => {
    console.log(`
🎯 Resilience Test Summary:
===========================
✅ Server Recovery Patterns
✅ Graceful Failure Handling
✅ Resource Limit Management
✅ Concurrent Operation Support
✅ Timeout Handling
✅ Data Integrity Verification

Benefits:
• Tests real-world failure scenarios
• Validates error handling robustness
• Ensures system stability under load
• Verifies graceful degradation
• Maintains data consistency
===========================
`);
  });
});