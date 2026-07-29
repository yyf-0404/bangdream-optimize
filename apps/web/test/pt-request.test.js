import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const calculationSource = await readFile(
  new URL('../src/actions/calculation.js', import.meta.url),
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
