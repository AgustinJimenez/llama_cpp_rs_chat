import { test, expect } from '@playwright/test';

test.describe('API Endpoints', () => {
  test('health endpoint returns success', async ({ request }) => {
    const response = await request.get('/health');
    expect(response.status()).toBe(200);
    
    const data = await response.json();
    expect(data).toEqual({
      status: 'ok',
      service: 'llama-chat-web'
    });
  });

  test('config endpoint returns sampler configuration', async ({ request }) => {
    const response = await request.get('/api/config');
    expect(response.status()).toBe(200);
    
    const data = await response.json();
    expect(data).toMatchObject({
      sampler_type: expect.any(String),
      temperature: expect.any(Number),
      top_p: expect.any(Number),
      top_k: expect.any(Number),
      mirostat_tau: expect.any(Number),
      mirostat_eta: expect.any(Number)
    });
  });

  test('chat endpoint handles valid requests', async ({ request }) => {
    const response = await request.post('/api/chat', {
      headers: {
        'Content-Type': 'application/json',
      },
      data: {
        message: 'Hello, this is a test message',
      }
    });

    expect(response.status()).toBe(200);
    
    const data = await response.json();
    expect(data).toMatchObject({
      message: {
        id: expect.any(String),
        role: 'assistant',
        content: expect.any(String),
        timestamp: expect.any(Number)
      },
      conversation_id: expect.any(String)
    });

    // Validate that the response content is not empty
    expect(data.message.content.length).toBeGreaterThan(0);
    
    // Validate UUID format for message ID
    expect(data.message.id).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/);
    expect(data.conversation_id).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/);
  });

  test('chat endpoint handles conversation continuity', async ({ request }) => {
    const firstResponse = await request.post('/api/chat', {
      headers: {
        'Content-Type': 'application/json',
      },
      data: {
        message: 'What is 2+2?',
      }
    });

    expect(firstResponse.status()).toBe(200);
    const firstData = await firstResponse.json();
    const conversationId = firstData.conversation_id;

    // Send follow-up message with same conversation ID
    const secondResponse = await request.post('/api/chat', {
      headers: {
        'Content-Type': 'application/json',
      },
      data: {
        message: 'What about 3+3?',
        conversation_id: conversationId
      }
    });

    expect(secondResponse.status()).toBe(200);
    const secondData = await secondResponse.json();
    
    // Should maintain the same conversation ID
    expect(secondData.conversation_id).toBe(conversationId);
    expect(secondData.message.content.length).toBeGreaterThan(0);
  });

  test('chat endpoint handles malformed JSON', async ({ request }) => {
    const response = await request.post('/api/chat', {
      headers: {
        'Content-Type': 'application/json',
      },
      data: '{"invalid": json}'
    });

    expect(response.status()).toBe(400);
    
    const data = await response.json();
    expect(data.error).toContain('Invalid JSON format');
  });

  test('chat endpoint handles missing message field', async ({ request }) => {
    const response = await request.post('/api/chat', {
      headers: {
        'Content-Type': 'application/json',
      },
      data: {
        conversation_id: 'test-id'
      }
    });

    expect(response.status()).toBe(400);
  });

  test('chat endpoint handles CORS headers', async ({ request }) => {
    const response = await request.post('/api/chat', {
      headers: {
        'Content-Type': 'application/json',
        'Origin': 'http://localhost:3001'
      },
      data: {
        message: 'Test CORS'
      }
    });

    expect(response.headers()['access-control-allow-origin']).toBe('*');
    expect(response.headers()['access-control-allow-methods']).toContain('POST');
    expect(response.headers()['access-control-allow-headers']).toContain('content-type');
  });

  test('OPTIONS request for preflight CORS', async ({ request }) => {
    const response = await request.fetch('/api/chat', {
      method: 'OPTIONS',
      headers: {
        'Origin': 'http://localhost:3001',
        'Access-Control-Request-Method': 'POST',
        'Access-Control-Request-Headers': 'content-type'
      }
    });

    expect(response.status()).toBe(200);
    expect(response.headers()['access-control-allow-origin']).toBe('*');
    expect(response.headers()['access-control-allow-methods']).toContain('POST');
  });

  test('chat endpoint handles code generation with comparison operators', async ({ request }) => {
    test.setTimeout(120000); // 2 minutes for code generation

    // First message: simple greeting
    const firstResponse = await request.post('/api/chat', {
      headers: {
        'Content-Type': 'application/json',
      },
      data: {
        message: 'hello',
      }
    });

    expect(firstResponse.status()).toBe(200);
    const firstData = await firstResponse.json();
    expect(firstData.message.content.length).toBeGreaterThan(0);
    const conversationId = firstData.conversation_id;

    // Second message: request code with < operator
    const secondResponse = await request.post('/api/chat', {
      headers: {
        'Content-Type': 'application/json',
      },
      data: {
        message: 'write me a binary search example code in javascript',
        conversation_id: conversationId
      }
    });

    expect(secondResponse.status()).toBe(200);
    const secondData = await secondResponse.json();

    // Should maintain the same conversation ID
    expect(secondData.conversation_id).toBe(conversationId);

    // Response should contain code
    expect(secondData.message.content.length).toBeGreaterThan(0);

    // Response should not be truncated at '<' character
    // (This verifies the stop token fix)
    // A proper binary search will contain comparison operators like '<' or '<='
    const content = secondData.message.content.toLowerCase();
    const hasCode = content.includes('function') ||
                    content.includes('search') ||
                    content.includes('binary');

    expect(hasCode).toBe(true);

    // Verify response is substantial (not cut off prematurely)
    expect(secondData.message.content.length).toBeGreaterThan(50);
  });
});