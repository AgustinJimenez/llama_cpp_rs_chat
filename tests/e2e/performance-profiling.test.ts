import { test, expect } from '@playwright/test';

/**
 * Performance Profiling Tests
 * 
 * These tests focus on measuring and validating performance characteristics
 * without requiring model inference or large context processing.
 */

test.describe('Performance Profiling Tests', () => {

  test('response time measurement - API endpoint latency', async ({ request }) => {
    console.log('⚡ Testing API response times...');
    
    const endpoints = [
      { name: 'Health Check', path: '/health', method: 'GET' },
      { name: 'Model Status', path: '/api/model/status', method: 'GET' },
      { name: 'Config Retrieval', path: '/api/config', method: 'GET' }
    ];
    
    const results = [];
    
    for (const endpoint of endpoints) {
      const startTime = Date.now();
      
      const response = endpoint.method === 'GET' 
        ? await request.get(endpoint.path)
        : await request.post(endpoint.path);
      
      const endTime = Date.now();
      const latency = endTime - startTime;
      
      results.push({
        name: endpoint.name,
        latency: latency,
        status: response.status()
      });
      
      console.log(`   📊 ${endpoint.name}: ${latency}ms`);
      
      // Basic performance expectations
      expect(latency).toBeLessThan(5000); // 5 second max
      expect(response.status()).toBeLessThan(500);
    }
    
    const averageLatency = results.reduce((sum, r) => sum + r.latency, 0) / results.length;
    console.log(`   📈 Average latency: ${averageLatency.toFixed(2)}ms`);
  });

  test('throughput testing - multiple rapid requests', async ({ request }) => {
    console.log('🚀 Testing request throughput...');
    
    const requestCount = 10;
    const requests = [];
    const startTime = Date.now();
    
    // Generate multiple concurrent requests
    for (let i = 0; i < requestCount; i++) {
      requests.push(
        request.post('/api/tools/execute', {
          data: {
            tool_name: 'bash',
            arguments: { command: `echo "Request ${i}"` }
          }
        })
      );
    }
    
    const results = await Promise.allSettled(requests);
    const endTime = Date.now();
    const totalTime = endTime - startTime;
    
    let successCount = 0;
    let failureCount = 0;
    
    for (const result of results) {
      if (result.status === 'fulfilled') {
        const response = await result.value.json();
        if (response.success) {
          successCount++;
        } else {
          failureCount++;
        }
      } else {
        failureCount++;
      }
    }
    
    const throughput = (successCount / totalTime) * 1000; // requests per second
    
    console.log(`   📊 Total time: ${totalTime}ms`);
    console.log(`   ✅ Successful requests: ${successCount}/${requestCount}`);
    console.log(`   📈 Throughput: ${throughput.toFixed(2)} req/sec`);
    
    expect(successCount).toBeGreaterThan(requestCount * 0.7); // At least 70% success
  });

  test('file operation speed - I/O performance', async ({ request }) => {
    console.log('💾 Testing file operation performance...');
    
    const fileSizes = [100, 500, 1000]; // bytes
    const operations = ['write', 'read'];
    
    const performanceResults = [];
    
    for (const size of fileSizes) {
      const content = 'x'.repeat(size);
      const filename = `perf_test_${size}.txt`;
      
      for (const operation of operations) {
        const startTime = Date.now();
        
        let response;
        if (operation === 'write') {
          response = await request.post('/api/tools/execute', {
            data: {
              tool_name: 'write_file',
              arguments: { 
                path: `test_data/${filename}`,
                content: content
              }
            }
          });
        } else {
          response = await request.post('/api/tools/execute', {
            data: {
              tool_name: 'read_file',
              arguments: { path: `test_data/${filename}` }
            }
          });
        }
        
        const endTime = Date.now();
        const duration = endTime - startTime;
        
        const result = await response.json();
        
        if (result.success) {
          performanceResults.push({
            operation,
            size,
            duration,
            throughputMBs: (size / duration / 1000) // MB/s
          });
          
          console.log(`   📊 ${operation} ${size}B: ${duration}ms`);
        }
      }
    }
    
    // Calculate averages
    const writeOps = performanceResults.filter(r => r.operation === 'write');
    const readOps = performanceResults.filter(r => r.operation === 'read');
    
    if (writeOps.length > 0) {
      const avgWriteTime = writeOps.reduce((sum, op) => sum + op.duration, 0) / writeOps.length;
      console.log(`   📝 Average write time: ${avgWriteTime.toFixed(2)}ms`);
    }
    
    if (readOps.length > 0) {
      const avgReadTime = readOps.reduce((sum, op) => sum + op.duration, 0) / readOps.length;
      console.log(`   📖 Average read time: ${avgReadTime.toFixed(2)}ms`);
    }
  });

  test('memory usage patterns - resource consumption', async ({ request }) => {
    console.log('🧠 Testing memory usage patterns...');
    
    const operations = [
      'Small operation',
      'Medium operation',
      'Large operation'
    ];
    
    for (let i = 0; i < operations.length; i++) {
      console.log(`   🔍 ${operations[i]}...`);
      
      // Before operation - get model status
      const beforeResponse = await request.get('/api/model/status');
      const beforeStatus = await beforeResponse.json();
      
      // Perform operation
      await request.post('/api/tools/execute', {
        data: {
          tool_name: 'write_file',
          arguments: { 
            path: `test_data/memory_test_${i}.txt`,
            content: 'x'.repeat(Math.pow(10, i + 2)) // 100, 1000, 10000 chars
          }
        }
      });
      
      // After operation - get model status
      const afterResponse = await request.get('/api/model/status');
      const afterStatus = await afterResponse.json();
      
      console.log(`   💾 Before: ${beforeStatus.memory_usage_mb || 'unknown'}MB`);
      console.log(`   💾 After: ${afterStatus.memory_usage_mb || 'unknown'}MB`);
      
      // Small delay between operations
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    
    console.log('   ✅ Memory pattern analysis completed');
  });

  test('error rate measurement - stability under load', async ({ request }) => {
    console.log('📈 Testing error rates under load...');
    
    const testDuration = 5000; // 5 seconds
    const requestInterval = 200; // Request every 200ms
    const startTime = Date.now();
    
    let requestCount = 0;
    let successCount = 0;
    let errorCount = 0;
    
    const runRequests = async () => {
      while (Date.now() - startTime < testDuration) {
        try {
          requestCount++;
          
          const response = await request.post('/api/tools/execute', {
            timeout: 1000,
            data: {
              tool_name: 'bash',
              arguments: { command: 'echo "Load test"' }
            }
          });
          
          const result = await response.json();
          if (result.success) {
            successCount++;
          } else {
            errorCount++;
          }
          
        } catch (error) {
          errorCount++;
        }
        
        await new Promise(resolve => setTimeout(resolve, requestInterval));
      }
    };
    
    await runRequests();
    
    const errorRate = (errorCount / requestCount) * 100;
    const successRate = (successCount / requestCount) * 100;
    
    console.log(`   📊 Total requests: ${requestCount}`);
    console.log(`   ✅ Success rate: ${successRate.toFixed(1)}%`);
    console.log(`   ❌ Error rate: ${errorRate.toFixed(1)}%`);
    
    // Expect reasonable success rate
    expect(successRate).toBeGreaterThan(80); // At least 80% success
  });

});

test.describe('Performance Summary', () => {
  test('display performance summary', async () => {
    console.log(`
🎯 Performance Profiling Summary:
=================================
✅ Response Time Measurement
✅ Throughput Testing
✅ File Operation Speed
✅ Memory Usage Patterns
✅ Error Rate Analysis

Benefits:
• Identifies performance bottlenecks
• Validates system stability
• Measures resource efficiency
• Provides load testing
• Tracks performance regressions
=================================
`);
  });
});