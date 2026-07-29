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

test('non-medley performances expose fire costs and PT/CP multipliers', () => {
  assert.deepEqual(ptResultMultiplierOptions('cooperative'), [
    { resource: 0, multiplier: 1 },
    { resource: 1, multiplier: 5 },
    { resource: 2, multiplier: 10 },
    { resource: 3, multiplier: 15 },
    { resource: 10, multiplier: 40 },
  ]);
});

test('medley performance selects fire per song and reports the three-song total', () => {
  assert.deepEqual(ptResultMultiplierOptions('medley'), [
    { resource: 0, perSongResource: 0, multiplier: 3 },
    { resource: 3, perSongResource: 1, multiplier: 15 },
    { resource: 6, perSongResource: 2, multiplier: 30 },
    { resource: 9, perSongResource: 3, multiplier: 45 },
  ]);
});

test('result multiplier scales the exact average before display rounding', () => {
  assert.equal(formatScaledAverageInteger(1_001, 2, 5), '2,503');
  assert.equal(formatScaledAverageFixed(1_001, 4, 5, 4), '1251.2500');
});
