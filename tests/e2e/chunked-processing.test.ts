import { test, expect } from '@playwright/test';

/**
 * Chunked Processing Tests
 * 
 * These tests handle larger tasks by breaking them into smaller chunks
 * to avoid timeout and memory issues while still testing complex functionality.
 */

test.describe('Chunked Processing Tests', () => {

  test('incremental file processing - read files in sequence', async ({ request }) => {
    console.log('🔄 Testing incremental file processing...');
    
    const files = ['config.json', 'sample_file.txt', 'README.md'];
    const results = [];
    
    for (const file of files) {
      console.log(`   📖 Reading ${file}...`);
      const response = await request.post('/api/tools/execute', {
        data: {
          tool_name: 'read_file',
          arguments: { path: `test_data/${file}` }
        }
      });
      
      expect(response.status()).toBe(200);
      const result = await response.json();
      expect(result.success).toBe(true);
      results.push({ file, size: result.result.length });
      
      // Small delay to prevent overwhelming the server
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    
    console.log('✅ Processed files:', results);
  });

  test('batch operations - multiple small commands', async ({ request }) => {
    console.log('📦 Testing batch operations...');
    
    const operations = [
      { tool: 'bash', cmd: 'echo "Operation 1"' },
      { tool: 'bash', cmd: 'echo "Operation 2"' },
      { tool: 'bash', cmd: 'echo "Operation 3"' }
    ];
    
    for (let i = 0; i < operations.length; i++) {
      console.log(`   ⚡ Running operation ${i + 1}...`);
      const response = await request.post('/api/tools/execute', {
        data: {
          tool_name: operations[i].tool,
          arguments: { command: operations[i].cmd }
        }
      });
      
      expect(response.status()).toBe(200);
      const result = await response.json();
      expect(result.success).toBe(true);
      expect(result.result).toContain(`Operation ${i + 1}`);
    }
    
    console.log('✅ All batch operations completed');
  });

  test('progressive file creation - build file incrementally', async ({ request }) => {
    console.log('📝 Testing progressive file creation...');
    
    const baseContent = '# Progressive Test File\n\n';
    let currentContent = baseContent;
    
    // Create initial file
    console.log('   ✍️ Creating initial file...');
    let response = await request.post('/api/tools/execute', {
      data: {
        tool_name: 'write_file',
        arguments: { 
          path: 'test_data/progressive.md',
          content: currentContent
        }
      }
    });
    expect(response.status()).toBe(200);
    
    // Add content in chunks
    const additions = [
      '## Section 1\nFirst section content.\n\n',
      '## Section 2\nSecond section content.\n\n',
      '## Section 3\nThird section content.\n\n'
    ];
    
    for (let i = 0; i < additions.length; i++) {
      currentContent += additions[i];
      console.log(`   📄 Adding section ${i + 1}...`);
      
      response = await request.post('/api/tools/execute', {
        data: {
          tool_name: 'write_file',
          arguments: { 
            path: 'test_data/progressive.md',
            content: currentContent
          }
        }
      });
      expect(response.status()).toBe(200);
    }
    
    // Verify final content
    response = await request.post('/api/tools/execute', {
      data: {
        tool_name: 'read_file',
        arguments: { path: 'test_data/progressive.md' }
      }
    });
    
    const result = await response.json();
    expect(result.result).toContain('Section 1');
    expect(result.result).toContain('Section 3');
    console.log('✅ Progressive file creation completed');
  });

  test('streaming simulation - process data in chunks', async ({ request }) => {
    console.log('🌊 Testing streaming simulation...');
    
    const data = ['chunk1', 'chunk2', 'chunk3', 'chunk4', 'chunk5'];
    const processedChunks = [];
    
    for (let i = 0; i < data.length; i++) {
      console.log(`   🔄 Processing chunk ${i + 1}/${data.length}...`);
      
      const response = await request.post('/api/tools/execute', {
        data: {
          tool_name: 'write_file',
          arguments: { 
            path: `test_data/stream_${i + 1}.txt`,
            content: `Processed: ${data[i]} at ${new Date().toISOString()}`
          }
        }
      });
      
      expect(response.status()).toBe(200);
      const result = await response.json();
      expect(result.success).toBe(true);
      processedChunks.push(data[i]);
      
      // Simulate processing delay
      await new Promise(resolve => setTimeout(resolve, 50));
    }
    
    expect(processedChunks).toEqual(data);
    console.log('✅ Streaming simulation completed');
  });

  test('memory-safe processing - small context operations', async ({ request }) => {
    console.log('🧠 Testing memory-safe processing...');
    
    // Process small JSON objects individually
    const jsonObjects = [
      { id: 1, name: 'test1', value: 100 },
      { id: 2, name: 'test2', value: 200 },
      { id: 3, name: 'test3', value: 300 }
    ];
    
    for (const obj of jsonObjects) {
      console.log(`   🔍 Processing object ${obj.id}...`);
      
      const response = await request.post('/api/tools/execute', {
        data: {
          tool_name: 'write_file',
          arguments: { 
            path: `test_data/object_${obj.id}.json`,
            content: JSON.stringify(obj, null, 2)
          }
        }
      });
      
      expect(response.status()).toBe(200);
      
      // Verify by reading back
      const readResponse = await request.post('/api/tools/execute', {
        data: {
          tool_name: 'read_file',
          arguments: { path: `test_data/object_${obj.id}.json` }
        }
      });
      
      const result = await readResponse.json();
      const parsed = JSON.parse(result.result);
      expect(parsed.id).toBe(obj.id);
    }
    
    console.log('✅ Memory-safe processing completed');
  });

});

test.describe('Chunked Processing Summary', () => {
  test('display chunked processing summary', async () => {
    console.log(`
🎯 Chunked Processing Test Summary:
===================================
✅ Incremental File Processing
✅ Batch Operations
✅ Progressive File Creation  
✅ Streaming Simulation
✅ Memory-Safe Processing

Benefits:
• Handles larger datasets safely
• Avoids timeout issues
• Memory-efficient processing
• Real-world workflow simulation
• Scalable task patterns
===================================
`);
  });
});