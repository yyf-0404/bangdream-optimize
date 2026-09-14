import test from 'node:test';
import assert from 'node:assert/strict';
import { createBrowserRuntime } from '../src/runtime/browser.js';
import { createProfileActions } from '../src/actions/profile.js';
import { importSourceMarkup } from '../src/ui/approved/archive-flows.js';
import { flowHelpers } from '../src/ui/archive-flows.js';

const servers = ['cn', 'jp', 'en', 'tw', 'kr'];

test('all regions, including CN, read public Bestdori profiles through the same browser adapter', async () => {
  const previous = {fetch: globalThis.fetch, indexedDB: globalThis.indexedDB, config: globalThis.BANGDREAM_OPTIMIZE_CONFIG};
  const requests = [];
  const profile = {mainUserDeck: {leader: 30}};
  globalThis.indexedDB = {open() {const request = {result: {}}; queueMicrotask(() => request.onsuccess()); return request;}};
  globalThis.BANGDREAM_OPTIMIZE_CONFIG = {apiBaseUrl: ''};
  globalThis.fetch = async (url, options) => {requests.push({url, options}); return new Response(JSON.stringify({result: true, data: {profile}}));};
  try {
    const runtime = await createBrowserRuntime();
    assert.equal(runtime.importBangDreamUserData, undefined);
    for (const server of servers) {
      assert.deepEqual(await runtime.importBestdoriPlayerProfile({playerId: 123, server}), profile);
    }
    assert.deepEqual(requests.map(r => r.url), servers.map(server => `/bestdori/player/${server}/123?mode=3`));
    assert.ok(requests.every(r => r.options.cache === 'no-cache' && !r.options.body));
  } finally {
    globalThis.fetch = previous.fetch;
    if (previous.indexedDB === undefined) delete globalThis.indexedDB; else globalThis.indexedDB = previous.indexedDB;
    if (previous.config === undefined) delete globalThis.BANGDREAM_OPTIMIZE_CONFIG; else globalThis.BANGDREAM_OPTIMIZE_CONFIG = previous.config;
  }
});

test('profile account actions use Bestdori for CN and leave the archive unchanged on read failure', async () => {
  const inputClass = globalThis.HTMLInputElement, selectClass = globalThis.HTMLSelectElement;
  globalThis.HTMLInputElement = class {};
  globalThis.HTMLSelectElement = class {};
  try {
    for (const server of servers) {
      const player = {server, playerId: 123, cardList: {9: {skillLevel: 5}}};
      const requests = [], errors = [], writes = [];
      const failure = new Error('Bestdori unavailable');
      const actions = createProfileActions({
        state: {activePlayerProfileId: 'a', playerProfiles: [{id: 'a'}], runtime: {
          importBestdoriPlayerProfile: async args => {requests.push(args); throw failure;},
        }}, elements: {}, normalizedPlayer: structuredClone, normalizedServer: s => s,
        parseEntityId: Number, ensureCore: async () => {}, readPlayer: () => structuredClone(player),
        writePlayer: p => writes.push(p), setStatus() {}, setError: e => errors.push(e),
      });
      await actions.handleImportMainBand();
      assert.deepEqual(requests, [{playerId: 123, server, mode: 3}]);
      assert.deepEqual(errors, [failure]);
      assert.deepEqual(writes, []);
    }
  } finally {
    if (inputClass === undefined) delete globalThis.HTMLInputElement; else globalThis.HTMLInputElement = inputClass;
    if (selectClass === undefined) delete globalThis.HTMLSelectElement; else globalThis.HTMLSelectElement = selectClass;
  }
});

test('CN account import advertises public main-band scope and keeps configuration paste available', () => {
  for (const server of servers) {
    const markup = importSourceMarkup({...flowHelpers, d: {source: 'account', server, playerId: '', format: 'base64'}, p: {server, name: 'Test'}});
    assert.match(markup, /仅导入主乐队的公开资料/);
    assert.match(markup, /公开资料不包含完整持有卡牌列表/);
    assert.match(markup, /value="cn"/);
    assert.match(markup, /data-source="paste"/);
    assert.doesNotMatch(markup, /读取国服账号配置/);
  }
});
