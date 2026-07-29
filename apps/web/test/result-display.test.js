import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const resultViewSource = await readFile(
  new URL('../src/views/result.js', import.meta.url),
  'utf8',
);
const indexSource = await readFile(
  new URL('../index.html', import.meta.url),
  'utf8',
);
const stylesSource = await readFile(
  new URL('../styles.css', import.meta.url),
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
  assert.match(resultViewSource, /segmented-control result-multiplier-control/);
  assert.match(resultViewSource, /\$\{option\.resource\} CP \/ \$\{option\.multiplier\} 倍/);
  assert.match(resultViewSource, /0, multiplier: 1/);
  assert.match(resultViewSource, /3, multiplier: 15/);
  assert.match(resultViewSource, /10, multiplier: 40/);
  assert.match(resultViewSource, /\$\{option\.resource\} 火 \/ \$\{option\.multiplier\} 倍/);
  assert.match(resultViewSource, /perSongResource: 3, multiplier: 45/);
  assert.match(resultViewSource, /每曲倍率选择/);
  assert.match(resultViewSource, /result-multiplier-control-medley/);
  assert.match(
    resultViewSource,
    /每曲 \$\{option\.perSongResource\} 火 \/ \$\{option\.multiplier\} 倍/,
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
  assert.match(resultViewSource, /title\.textContent = '道具选择'/);
  assert.match(resultViewSource, /result-section result-items-section/);
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

test('segmented controls use context-specific fixed widths and stay left aligned', () => {
  const segmentedBlock = stylesSource.match(/\.segmented-control\s*\{([^}]*)\}/)?.[1] ?? '';
  assert.match(segmentedBlock, /display: flex;/);
  assert.match(segmentedBlock, /justify-self: start;/);
  assert.match(segmentedBlock, /border: 0;/);
  assert.match(segmentedBlock, /box-shadow: inset 0 0 0 1px var\(--line\);/);
  assert.doesNotMatch(segmentedBlock, /grid-auto-columns/);
  assert.match(stylesSource, /flex: 0 0 var\(--segment-width\)/);
  assert.match(stylesSource, /\.segmented-control > label[\s\S]*min-height: 38px;/);
  assert.match(stylesSource, /\.calculation-mode-control\s*\{\s*--segment-width: 132px;/);
  assert.match(stylesSource, /#pt-maximize-versus-rank,[\s\S]*--segment-width: 44px;/);
  assert.match(stylesSource, /\.result-multiplier-control\s*\{\s*--segment-width: 132px;/);
  assert.match(
    stylesSource,
    /\.result-multiplier-control-medley\s*\{[\s\S]*grid-template-columns: repeat\(4, minmax\(0, 1fr\)\);[\s\S]*width: min\(100%, 696px\);[\s\S]*overflow: hidden;/,
  );
  assert.match(
    stylesSource,
    /\.result-multiplier-control-medley > label > span\s*\{[\s\S]*white-space: nowrap;/,
  );
  assert.match(
    stylesSource,
    /@container \(max-width: 580px\)\s*\{[\s\S]*\.result-multiplier-control-medley\s*\{[\s\S]*grid-template-columns: repeat\(2, minmax\(0, 1fr\)\);/,
  );
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
