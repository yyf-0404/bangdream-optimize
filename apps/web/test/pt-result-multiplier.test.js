import assert from 'node:assert/strict';
import test from 'node:test';

import {
  formatScaledAverageFixed,
  formatScaledAverageInteger,
  ptResultMultiplierOptions,
} from '../src/views/result.js';

test('challenge performance exposes CP costs and PT multipliers', () => {
  assert.deepEqual(ptResultMultiplierOptions('challenge_cp'), [
    { resource: 200, multiplier: 1 },
    { resource: 400, multiplier: 2 },
    { resource: 800, multiplier: 4 },
    { resource: 1600, multiplier: 8 },
  ]);
});

test('other performances expose fire costs and PT/CP multipliers', () => {
  assert.deepEqual(ptResultMultiplierOptions('cooperative'), [
    { resource: 0, multiplier: 1 },
    { resource: 1, multiplier: 5 },
    { resource: 2, multiplier: 10 },
    { resource: 3, multiplier: 15 },
  ]);
});

test('result multiplier scales the exact average before display rounding', () => {
  assert.equal(formatScaledAverageInteger(1_001, 2, 5), '2,503');
  assert.equal(formatScaledAverageFixed(1_001, 4, 5, 4), '1251.2500');
});
