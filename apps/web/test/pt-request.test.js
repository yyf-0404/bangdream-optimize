import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const calculationSource = await readFile(
  new URL('../src/actions/calculation.js', import.meta.url),
  'utf8',
);
const pageSource = await readFile(
  new URL('../src/app/page.js', import.meta.url),
  'utf8',
);

test('PT maximize request keeps the selected event type', () => {
  const requestBuilder = calculationSource.match(
    /function readPtMaximizeRequest[\s\S]*?\n  function /,
  )?.[0] ?? '';
  assert.match(requestBuilder, /const eventType = ptMaximizeEventType\(player, eventId\)/);
  assert.match(requestBuilder, /const request = \{\s*eventType,\s*liveVariant,/);
  assert.doesNotMatch(requestBuilder, /eventType:\s*'challenge'/);
});

test('mission live cooperative request includes the support team PT bonus', () => {
  const requestBuilder = calculationSource.match(
    /function readPtMaximizeRequest[\s\S]*?\n  function /,
  )?.[0] ?? '';
  assert.match(
    requestBuilder,
    /eventType === 'mission_live'[\s\S]*liveVariant === 'solo'[\s\S]*liveVariant === 'cooperative'/,
  );
  assert.match(
    requestBuilder,
    /missionSupportPtBonus: form\.missionSupportPtBonus \?\? 100/,
  );
  assert.match(
    pageSource,
    /event\?\.eventType === 'mission_live'[\s\S]*selected === 'solo'[\s\S]*selected === 'cooperative'[\s\S]*ptMaximizeMissionSupportField\.hidden = !missionLive/,
  );
});

test('calculation yields a paint before starting synchronous preparation', () => {
  const calculateHandler = calculationSource.match(
    /async function handleCalculate[\s\S]*?\n  async function calculateScoreRange/,
  )?.[0] ?? '';
  assert.match(
    calculateHandler,
    /setCalculatingState\(true\);[\s\S]*setStatus\('准备计算'\);\s*await yieldForPaint\(\);[\s\S]*ensureCore/,
  );
});
