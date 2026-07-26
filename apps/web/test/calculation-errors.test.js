import assert from 'node:assert/strict';
import test from 'node:test';

import {
  explainCalculationError,
  scoreRangeEmptyExplanation,
} from '../src/data/calculation-errors.js';
import { createDiagnostics } from '../src/data/diagnostics.js';

test('classifies an insufficient card pool', () => {
  const explanation = explainCalculationError(
    new Error('single-song error: at least five cards are required to build a team, got 3'),
  );
  assert.equal(explanation.category, 'configuration');
  assert.equal(explanation.code, 'insufficient-cards');
  assert.equal(explanation.title, '可用卡牌不足');
  assert.match(explanation.detail, /当前计算只找到 3 张/);
});

test('uses cooperative minimum stat context for an otherwise generic no-result error', () => {
  const explanation = explainCalculationError(
    new Error('PT-maximize error: no valid PT-maximizing team was found'),
    {
      player: { calculationMode: 'ptMaximize' },
      calculationRequest: {
        liveVariant: 'cooperative',
        minimumPersonalStat: 500000,
      },
    },
  );
  assert.equal(explanation.code, 'minimum-stat-unreachable');
  assert.equal(explanation.title, '最低综合力限制无法满足');
  assert.match(explanation.detail, /500,000/);
});

test('classifies score-range targets below current PT', () => {
  const explanation = explainCalculationError(
    new Error('target total PT 100 is below current PT 200'),
  );
  assert.equal(explanation.code, 'score-range-target-too-low');
  assert.equal(explanation.title, '目标 PT 设置过低');
});

test('classifies invalid song counts and missing chart data', () => {
  assert.equal(
    explainCalculationError(
      new Error('Medley PT-maximize requires exactly three songs, got 2'),
    ).code,
    'invalid-song-count',
  );
  assert.equal(
    explainCalculationError(new Error('chart id 100:4 is missing')).code,
    'missing-game-data',
  );
});

test('separates missing mode input from an unsupported mode', () => {
  assert.equal(
    explainCalculationError(
      new Error('input for live variant Cooperative is missing'),
    ).code,
    'missing-live-variant-input',
  );
  assert.equal(
    explainCalculationError(
      new Error('event type Versus does not support live variant Cooperative'),
    ).category,
    'unsupported',
  );
});

test('classifies invalid fire multipliers as configuration errors', () => {
  const explanation = explainCalculationError(
    new Error('fire multiplier 8 is invalid'),
  );
  assert.equal(explanation.category, 'configuration');
  assert.equal(explanation.code, 'invalid-fire-multiplier');
});

test('empty score-range result explains a possibly too-small PT increment', () => {
  const explanation = scoreRangeEmptyExplanation({
    currentPt: 1000,
    targetTotalPt: 1050,
  });
  assert.match(explanation.detail, /当前目标增量 50 PT/);
  assert.match(explanation.suggestion, /提高目标总 PT/);
});

test('failure diagnostics retain both the raw error and the user-facing explanation', async () => {
  const diagnostics = createDiagnostics({
    getRuntime: () => ({ kind: 'browser' }),
    getCore: () => null,
    appendLog() {},
  });
  const diagnostic = await diagnostics.buildDiagnostic({
    player: {
      currentEvent: 1,
      calculationMode: 'ptMaximize',
      cardList: {},
    },
    server: 'cn',
    eventId: 1,
    error: new Error('PT-maximize error: no valid PT-maximizing team was found'),
    calculationRequest: {
      liveVariant: 'cooperative',
      minimumPersonalStat: 500000,
    },
  });

  assert.equal(diagnostic.error.message, 'PT-maximize error: no valid PT-maximizing team was found');
  assert.equal(diagnostic.error.code, 'minimum-stat-unreachable');
  assert.equal(diagnostic.error.category, 'configuration');
  assert.equal(diagnostic.calculationRequest.minimumPersonalStat, 500000);
});
