import assert from 'node:assert/strict';
import test from 'node:test';
import { skillQueuePresentation } from '../src/views/result.js';

const deps = { songLabel: id => `Song ${id}` };
const notice = (kind, searchMayBeSuboptimal = false) => ({ songId:186, difficulty:2, kind, searchMayBeSuboptimal });

test('highest score and average PT share the queued notice', () => {
  for (const extra of [{ totalScore:123 }, { team:{evaluation:{averagePt:{}}} }, { medley:{averagePt:{}} }]) {
    const result = skillQueuePresentation({ ...extra, skillQueueNotices:[notice('single')] },deps);
    assert.equal(result.title,'计入技能延后');
    assert.equal(result.warning,false);
    assert.match(result.detail,/Song 186.*HARD/);
  }
});

test('chained automatic search warns about missing a better team', () => {
  const result = skillQueuePresentation({ skillQueueNotices:[notice('chain',true)] },deps);
  assert.equal(result.warning,true);
  assert.match(result.title,/可能不是最优解/);
  assert.match(result.detail,/完整排队时间线计算/);
});

test('state-covered chained search keeps an informational queue notice', () => {
  const result = skillQueuePresentation({ skillQueueNotices:[notice('chain',false)] },deps);
  assert.equal(result.title,'计入技能延后');
  assert.equal(result.warning,false);
  assert.doesNotMatch(result.detail,/遗漏更优|未完整覆盖/);
});

test('specified teams including chains never receive an optimization warning', () => {
  for (const mode of ['manual','auto']) {
    const result = skillQueuePresentation({ scoreMode:{mode}, skillQueueNotices:[notice('chain',true)] },deps);
    assert.equal(result.warning,false);
    assert.equal(result.detail,'Song 186 · ID 186 · HARD。计算已计入技能延后触发的影响。');
    assert.doesNotMatch(result.detail,/遗漏更优/);
  }
});

test('empty results stay quiet and legacy queued results ask for recalculation', () => {
  for (const value of [null,[],{}, { skillQueueNotices:[] }]) assert.equal(skillQueuePresentation(value,deps),null);
  const legacy = skillQueuePresentation({songs:[{skillQueueRisk:true}]},deps);
  assert.match(legacy.detail,/请重新计算/);
  assert.doesNotMatch(legacy.detail,/矩阵|直接相加/);
});
