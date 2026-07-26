import assert from 'node:assert/strict';
import test from 'node:test';

import { submitFeedbackRequest } from '../src/data/feedback.js';

test('feedback request uses the configured backend and returns its id', async () => {
  const originalFetch = globalThis.fetch;
  let captured;
  globalThis.fetch = async (url, options) => {
    captured = { url, options };
    return new Response(JSON.stringify({
      status: 'ok',
      data: { feedbackId: 'F-123' },
    }), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    });
  };
  try {
    const payload = {
      category: 'problem',
      subject: '测试',
      content: '内容',
    };
    const result = await submitFeedbackRequest({
      apiBaseUrl: 'https://calc.example.com/',
      payload,
    });

    assert.deepEqual(result, { feedbackId: 'F-123' });
    assert.equal(captured.url, 'https://calc.example.com/api/feedback');
    assert.equal(captured.options.method, 'POST');
    assert.ok(captured.options.body instanceof FormData);
    assert.deepEqual(JSON.parse(captured.options.body.get('payload')), payload);
    assert.equal(captured.options.headers, undefined);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test('feedback request appends user files and diagnostics', async () => {
  const originalFetch = globalThis.fetch;
  let captured;
  globalThis.fetch = async (url, options) => {
    captured = { url, options };
    return new Response(JSON.stringify({
      status: 'ok',
      data: { feedbackId: 'F-attachment' },
    }), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    });
  };
  try {
    const attachment = new File(['{"diagnostic":true}'], 'diagnostic.json', {
      type: 'application/json',
    });
    await submitFeedbackRequest({
      apiBaseUrl: '',
      payload: { category: 'problem', subject: '结果异常', content: '见附件' },
      attachments: [attachment],
    });

    const uploaded = captured.options.body.getAll('attachment');
    assert.equal(uploaded.length, 1);
    assert.equal(uploaded[0].name, 'diagnostic.json');
    assert.equal(await uploaded[0].text(), '{"diagnostic":true}');
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test('desktop feedback requires an explicit backend address', async () => {
  await assert.rejects(
    () => submitFeedbackRequest({
      apiBaseUrl: '',
      payload: {},
      requireConfiguredBase: true,
    }),
    /feedbackApiBaseUrl/,
  );
});

test('feedback error keeps the server message', async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async () => new Response(JSON.stringify({
    status: 'error',
    message: '提交过于频繁',
  }), {
    status: 429,
    headers: { 'Content-Type': 'application/json' },
  });
  try {
    await assert.rejects(
      () => submitFeedbackRequest({ apiBaseUrl: '', payload: {} }),
      /提交过于频繁/,
    );
  } finally {
    globalThis.fetch = originalFetch;
  }
});
