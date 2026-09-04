import assert from 'node:assert/strict';
import test from 'node:test';

import { nextCyclicValue } from '../src/ui/cycle-select.js';

test('cyclic selections share one wraparound primitive', () => {
  assert.equal(nextCyclicValue(['a', 'b', 'c'], 'a'), 'b');
  assert.equal(nextCyclicValue(['a', 'b', 'c'], 'c'), 'a');
  assert.equal(nextCyclicValue(['a', 'b', 'c'], 'a', -1), 'c');
  assert.equal(nextCyclicValue(['a', 'b', 'c'], 'c', -1), 'b');
  assert.equal(nextCyclicValue(['a', 'b', 'c'], 'missing'), 'a');
  assert.equal(nextCyclicValue([], 'a'), undefined);
});
