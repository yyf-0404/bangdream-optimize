import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const resultViewSource = await readFile(
  new URL('../src/pages/result/result-view.js', import.meta.url),
  'utf8',
);
const indexSource = await readFile(
  new URL('../index.html', import.meta.url),
  'utf8',
);

test('PT maximize summary shows integer averages without ranges or sample counts', () => {
  assert.match(resultViewSource, /resultStat\('平均活动 PT'/);
  assert.match(resultViewSource, /resultStat\('平均分数'/);
  assert.match(resultViewSource, /Math\.round\(numerator \/ denominator\)/);
  assert.doesNotMatch(resultViewSource, /resultStat\('PT 范围'/);
  assert.doesNotMatch(resultViewSource, /resultStat\('排列样本数'/);
  assert.doesNotMatch(resultViewSource, /resultStat\('样本数'/);
});

test('specified-team songs show cover, team and an open score range', () => {
  assert.match(resultViewSource, /composition-song-info/);
  assert.match(resultViewSource, /range.open=true/);
  assert.match(resultViewSource, /最低分数/);
  assert.match(resultViewSource, /平均分数/);
  assert.match(resultViewSource, /最高分数/);
  assert.match(resultViewSource, /song.scoreDistribution.minScore/);
  assert.match(resultViewSource, /song.scoreDistribution.maxScore/);
  assert.match(resultViewSource, /formatScoreDistributionAverage/);
});

test('PT maximize songs reuse score result cards while labeling cards as a team', () => {
  assert.match(resultViewSource, /renderPtMaximizeSongSection/);
  assert.match(resultViewSource, /renderSongResult\(song, index, maxScore, deps/);
  assert.match(resultViewSource, /skillTitle: '队伍'/);
  assert.match(resultViewSource, /showSkillOrder: false/);
});

test('PT maximize result renders fixed scenario context', () => {
  assert.match(resultViewSource, /renderPtScenario\(result\)/);
  assert.match(resultViewSource, /title\.textContent = '计算场景'/);
  assert.match(resultViewSource, /diagnosticItem\('演出模式'/);
  assert.doesNotMatch(resultViewSource, /diagnosticItem\('Fever'/);
  assert.doesNotMatch(resultViewSource, /diagnosticItem\('最低综合力'/);
  assert.doesNotMatch(resultViewSource, /diagnosticItem\('队友参数'/);
  assert.match(resultViewSource, /'排名'/);
  assert.match(resultViewSource, /'队伍结果'/);
});

test('result metrics only display total elapsed time', () => {
  assert.match(resultViewSource, /term\.textContent = '总耗时'/);
  assert.match(resultViewSource, /formatMs\(metrics\.totalElapsedMs\)/);
  assert.doesNotMatch(resultViewSource, /metrics\.single\.candidateBuildMs/);
  assert.doesNotMatch(resultViewSource, /metrics\.medley\.solveMs/);
});

test('PT maximize result includes challenge CP, regular fire, and per-song medley selectors', () => {
  assert.match(resultViewSource, /resource: 200, multiplier: 1/);
  assert.match(resultViewSource, /resource-options result-multiplier-control/);
  assert.match(resultViewSource, /\$\{option\.resource\} CP · ×\$\{option\.multiplier\}/);
  assert.match(resultViewSource, /0, multiplier: 1/);
  assert.match(resultViewSource, /3, multiplier: 15/);
  assert.match(resultViewSource, /10, multiplier: 40/);
  assert.match(resultViewSource, /\$\{option\.resource\} 火 · ×\$\{option\.multiplier\}/);
  assert.match(resultViewSource, /perSongResource: 3, multiplier: 45/);
  assert.match(resultViewSource, /组曲每曲火倍率/);
  assert.match(resultViewSource, /result-multiplier-control-medley/);
  assert.match(
    resultViewSource,
    /每曲 \$\{option\.perSongResource\} 火 · ×\$\{option\.multiplier\}/,
  );
  assert.match(resultViewSource, /formatScaledAverageInteger/);
  assert.match(resultViewSource, /formatScaledAverageFixed/);
});

test('PT maximize result places scenario and multiplier before its overview', () => {
  assert.match(
    resultViewSource,
    /resultElement\.append\(\s*renderPtScenario\(result\),\s*multiplierSelector,\s*overview,/,
  );
});

test('result item selection has its own heading', () => {
  assert.match(resultViewSource, /heading\.id='result-equipment-heading'/);
  assert.match(resultViewSource, /designIcon\('equipment'\)\+'道具选择'/);
  assert.match(resultViewSource, /section.className='item-selection'/);
});

test('teammate parameters follow the leader selection at the same section level', () => {
  const cooperative = indexSource.indexOf('id="pt-maximize-cooperative-fields"');
  const leader = indexSource.indexOf(
    '<legend class="pt-parameter-subheading">队长选择</legend>',
    cooperative,
  );
  const teammates = indexSource.indexOf(
    '<h3 class="pt-parameter-subheading">队友参数</h3>',
    leader,
  );
  const teammateMode = indexSource.indexOf('id="pt-maximize-teammate-mode"', teammates);
  assert.ok(cooperative >= 0);
  assert.ok(leader > cooperative);
  assert.ok(teammates > leader);
  assert.ok(teammateMode > teammates);
});

test('result feedback uses the inverse action style', () => {
  assert.match(
    indexSource,
    /id="feedback-result" class="inverse-action"/,
  );
});

test('calculation failures show a classified reason and a recovery suggestion', () => {
  assert.match(resultViewSource, /resultStat\('原因', error\.title/);
  assert.match(resultViewSource, /diagnosticItem\('问题说明'/);
  assert.match(resultViewSource, /diagnosticItem\('处理建议'/);
  assert.match(resultViewSource, /scoreRangeEmptyExplanation\(diagnostic\?\.calculationRequest\)/);
});
