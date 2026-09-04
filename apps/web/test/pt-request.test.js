import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const calculationSource = await readFile(
  new URL('../src/actions/calculation.js', import.meta.url),
  'utf8',
);
const activitySource = await readFile(
  new URL('../src/actions/activity.js', import.meta.url),
  'utf8',
);
const pageSource = await readFile(
  new URL('../src/app/page.js', import.meta.url),
  'utf8',
);
const indexSource = await readFile(
  new URL('../index.html', import.meta.url),
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

test('specified-team validation runs before entering the calculating state', () => {
  const calculateHandler = calculationSource.match(
    /async function handleCalculate[\s\S]*?\n  async function calculateScoreRange/,
  )?.[0] ?? '';
  const validationIndex = calculateHandler.indexOf('validatePtEvaluateBeforeCalculation()');
  const calculatingIndex = calculateHandler.indexOf('isCalculating = true');
  assert.ok(validationIndex >= 0);
  assert.ok(calculatingIndex > validationIndex);
});

test('specified-team request fixes the captain to the third card slot', () => {
  const requestBuilder = calculationSource.match(
    /function readPtEvaluateRequest[\s\S]*?\n  function /,
  )?.[0] ?? '';
  assert.match(requestBuilder, /const teamCount = liveVariant === 'medley' \? 3 : 1/);
  assert.match(requestBuilder, /captainCardId: cardIds\[2\]/);
  assert.match(requestBuilder, /scoreMode: form\.scoreMode === 'auto'/);
});

test('specified-team Auto is forced to manual outside free and medley lives', () => {
  const formReader = calculationSource.match(
    /function readPtEvaluateForm[\s\S]*?\n  function /,
  )?.[0] ?? '';
  assert.match(
    formReader,
    /ptEvaluateSupportsAuto\(liveVariant\) \? selectedScoreMode : 'manual'/,
  );
  assert.match(pageSource, /const autoAllowed = ptEvaluateSupportsAuto\(selected\)/);
});

test('specified teams use card previews while area items use reusable cycling selects', () => {
  assert.match(pageSource, /cardPreviewContent\(\{/);
  assert.match(pageSource, /className = 'pt-evaluate-card-choice'/);
  assert.doesNotMatch(pageSource, /pt-evaluate-item-choice/);
  assert.match(indexSource, /id="pt-evaluate-item-panel"/);
  assert.match(indexSource, /id="pt-evaluate-team-panel"/);
  assert.match(indexSource, /id="pt-evaluate-band-item"/);
  assert.match(indexSource, /id="pt-evaluate-attribute-item"/);
  assert.match(indexSource, /id="pt-evaluate-magazine-item"/);
  assert.match(indexSource, /id="pt-evaluate-band-item-cycle"/);
  assert.match(indexSource, /id="pt-evaluate-attribute-item-cycle"/);
  assert.match(indexSource, /id="pt-evaluate-magazine-item-cycle"/);
});

test('switching to specified-team calculation reports the correct status', () => {
  assert.match(
    activitySource,
    /calculationMode === 'ptEvaluate'\s*\? '已切换到指定队伍'/,
  );
});
