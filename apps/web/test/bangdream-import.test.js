import assert from 'node:assert/strict';
import test from 'node:test';

import {
  BANGDREAM_ACCOUNT_LOGIN_HINT,
  fetchBangDreamUserDataRequest,
} from '../src/data/bangdream-import.js';

async function withFetch(response, callback) {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async () => response;
  try {
    return await callback();
  } finally {
    globalThis.fetch = originalFetch;
  }
}

test('direct HTTP 405 uses the account login hint', async () => {
  await withFetch(new Response('', { status: 405 }), async () => {
    await assert.rejects(
      () => fetchBangDreamUserDataRequest({ apiBaseUrl: '', userId: 100 }),
      (error) => error.message === BANGDREAM_ACCOUNT_LOGIN_HINT,
    );
  });
});

test('proxied upstream HTTP 405 uses the account login hint', async () => {
  await withFetch(new Response(JSON.stringify({
    status: 'error',
    message: 'Bang Dream API returned HTTP 405: Unity login failed',
  }), {
    status: 502,
    headers: { 'Content-Type': 'application/json' },
  }), async () => {
    await assert.rejects(
      () => fetchBangDreamUserDataRequest({ apiBaseUrl: '', userId: 100 }),
      (error) => error.message === BANGDREAM_ACCOUNT_LOGIN_HINT,
    );
  });
});

test('successful account import returns player data', async () => {
  const data = { playerId: 100, cardList: {} };
  await withFetch(new Response(JSON.stringify({
    status: 'ok',
    data,
  }), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  }), async () => {
    assert.deepEqual(
      await fetchBangDreamUserDataRequest({ apiBaseUrl: '', userId: 100 }),
      data,
    );
  });
});

test('desktop account import still requires an explicit backend address', async () => {
  await assert.rejects(
    () => fetchBangDreamUserDataRequest({
      apiBaseUrl: '',
      userId: 100,
      requireConfiguredBase: true,
    }),
    /桌面端游戏账号导入需要配置后端 apiBaseUrl/,
  );
});
