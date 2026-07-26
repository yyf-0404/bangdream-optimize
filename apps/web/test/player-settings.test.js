import assert from 'node:assert/strict';
import test from 'node:test';

import {
  PLAYER_CONFIG_SCHEMA_VERSION,
  createDefaultPtMaximizeConfig,
  createDefaultScoreRangeConfig,
  normalizePtMaximizeConfig,
  normalizeScoreRangeConfig,
  ptMaximizeLiveVariant,
  withPtMaximizeLiveVariant,
} from '../src/models/player-settings.js';
import { samplePlayerConfig } from '../src/storage/user.js';

test('sample config and persisted setting defaults share one source', () => {
  const sample = samplePlayerConfig();
  assert.equal(sample.playerConfigVersion, PLAYER_CONFIG_SCHEMA_VERSION);
  assert.deepEqual(sample.scoreRange, createDefaultScoreRangeConfig('cn'));
  assert.deepEqual(sample.ptMaximize, createDefaultPtMaximizeConfig());
  assert.equal(sample.server, 'cn');
  assert.equal(sample.calculationMode, 'ptMaximize');
  assert.equal(sample.currentEvent, undefined);
  assert.deepEqual(sample.eventSongs, {});
  assert.equal(sample.ptMaximize.festivalWon, true);
  assert.equal(sample.ptMaximize.festivalTeammateScores[0], 4000000);
});

test('incomplete PT maximize configs are completed without sharing teammate objects', () => {
  const normalized = normalizePtMaximizeConfig({
    cooperativeLeaderMode: 'specified',
    cooperativeSpecifiedLeader: 99,
    teammates: [{ expectedStat: 310000 }],
  });

  assert.equal(normalized.cooperativeSpecifiedLeader, 4);
  assert.equal(normalized.festivalWon, true);
  assert.deepEqual(normalized.teammates[0], {
    expectedStat: 310000,
    leaderScoreUp: 130,
    leaderSkillDuration: 7,
  });
  normalized.teammates[0].expectedStat = 1;
  assert.equal(normalized.teammates[1].expectedStat, 290000);
});

test('invalid persisted values fall back to stable settings', () => {
  const normalized = normalizePtMaximizeConfig({
    liveVariant: 'festival',
    liveVariantByEventType: {
      challenge: 'festival',
      festival: 'festival',
    },
    minimumPersonalStat: -1,
    cooperativeLeaderMode: 'unknown',
    festivalWon: 'true',
    festivalTeammateScores: [null, -1, '5000000'],
  });

  assert.equal('liveVariant' in normalized, false);
  assert.equal(ptMaximizeLiveVariant(normalized, 'challenge'), 'solo');
  assert.equal(ptMaximizeLiveVariant(normalized, 'festival'), 'festival');
  assert.equal(normalized.minimumPersonalStat, 290000);
  assert.equal(normalized.cooperativeLeaderMode, 'max_stat');
  assert.equal(normalized.festivalWon, false);
  assert.deepEqual(normalized.festivalTeammateScores, [
    4000000,
    4000000,
    5000000,
    4000000,
  ]);
});

test('last live variant is stored independently for each event type', () => {
  let config = createDefaultPtMaximizeConfig();
  config = withPtMaximizeLiveVariant(config, 'challenge', 'challenge_cp');
  config = withPtMaximizeLiveVariant(config, 'festival', 'festival');

  assert.equal(ptMaximizeLiveVariant(config, 'challenge'), 'challenge_cp');
  assert.equal(ptMaximizeLiveVariant(config, 'festival'), 'festival');
  assert.equal(ptMaximizeLiveVariant(config, 'live_try'), 'solo');
  assert.equal(ptMaximizeLiveVariant(config, 'medley'), 'medley');
});

test('score range auto multiplier follows the current server only when absent', () => {
  assert.equal(normalizeScoreRangeConfig({}, 'jp').autoBaseMultiplier, 0.75);
  assert.equal(normalizeScoreRangeConfig({}, 'cn').autoBaseMultiplier, 0.5);
  assert.equal(
    normalizeScoreRangeConfig({ autoBaseMultiplier: 0.5 }, 'jp').autoBaseMultiplier,
    0.5,
  );
});
