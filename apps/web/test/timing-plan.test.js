import assert from 'node:assert/strict';
import test from 'node:test';
import {
  PLAN_REVISION, TimingPlanDraft, normalizeFrameRanges, parseFrameRanges, formatFrameRanges,
  makeTimingPlanRequest, timingPlanExport, planNoteRows,
  chartReferenceFrame, frameRangesToAbsolute, restoreFrameRanges, RANGE_COORDINATES,
  recomputeTimingPlans,
} from '../src/domain/timing-plan.js';
import {timingResult} from './fixtures/timing-result.js';
import {isCompleteTimingResult} from '../src/domain/hit-timing.js';
import {createViewAdapters} from '../src/app/view-adapters.js';

const range = (firstOffset = 1, lastOffset = 2) => ({noteIndex: 0, firstOffset, lastOffset});

test('result presentation receives the live runtime and profile used by range edits', () => {
  let presented, runtime = {kind: 'browser'}, profile = 'first';
  const adapters = createViewAdapters({
    elements: {resultSummary: {}}, readPlayer: () => ({}),
    getRuntime: () => runtime, getProfileId: () => profile,
    renderResultSummaryView: (_host, _result, dependencies) => { presented = dependencies; },
  });
  adapters.renderResultSummary(timingResult());
  assert.equal(presented.getRuntime(), runtime);
  assert.equal(presented.getProfileId(), 'first');
  runtime = {kind: 'desktop'}; profile = 'second';
  assert.equal(presented.getRuntime(), runtime);
  assert.equal(presented.getProfileId(), 'second');
});
function response(source, songIndex, frame, total = 90) {
  const result = structuredClone(source.hitTiming.songs[songIndex].result);
  result.hitFrames[0] = frame;
  result.total = total;
  Object.assign(result.trace[0], {frame, score: total, hitTime: frame / 120,
    offsetMs: (frame / 120 - source.hitTiming.songs[songIndex].input.notes[0].time) * 1000});
  return {planRevision: PLAN_REVISION, status: 'complete', result};
}

test('numbered inclusive ranges accept fixed frames and reject ambiguous or partial input', () => {
  const parsed = parseFrameRanges('#2 -3 +2\n\n1,0,0', 2);
  assert.deepEqual(parsed, [{noteIndex: 0, firstOffset: 0, lastOffset: 0}, {noteIndex: 1, firstOffset: -3, lastOffset: 2}]);
  assert.deepEqual(parseFrameRanges(formatFrameRanges(parsed), 2), parsed);
  assert.deepEqual(parseFrameRanges('', 2), []);
  for (const text of ['0 1 2', '3 1 2', '1 5 4', '1 2.5 3', '1 -72001 3', '1 2 72001', '1 2 3\n1 4 5', '1 2']) {
    assert.throws(() => parseFrameRanges(text, 2), undefined, text);
  }
  assert.throws(() => normalizeFrameRanges([{noteIndex: 0, firstOffset: NaN, lastOffset: 1}], 2));
  assert.throws(() => normalizeFrameRanges([{noteIndex: 0, firstFrame: 12, lastFrame: 13}], 2));
});

test('offsets anchor to the nearest valid chart reference and intersect only the global frame domain', () => {
  const notes = [{time:13.5 / 120}, {time:0.1}, {time:600}];
  assert.equal(chartReferenceFrame(notes[0]), 14);
  assert.deepEqual(frameRangesToAbsolute([
    {noteIndex:0,firstOffset:-3,lastOffset:2},
    {noteIndex:1,firstOffset:-20,lastOffset:3},
    {noteIndex:2,firstOffset:-1,lastOffset:5},
  ], notes), [
    {noteIndex:0,firstFrame:11,lastFrame:16},
    {noteIndex:1,firstFrame:0,lastFrame:15},
    {noteIndex:2,firstFrame:71999,lastFrame:72000},
  ]);
  assert.deepEqual(frameRangesToAbsolute([{noteIndex:0,firstOffset:0,lastOffset:0}],notes),
    [{noteIndex:0,firstFrame:14,lastFrame:14}]);
  assert.throws(() => frameRangesToAbsolute([{noteIndex:1,firstOffset:-20,lastOffset:-15}],notes), /没有交集/);
  const source = timingResult(), draft = new TimingPlanDraft(source, 0);
  draft.setRange(0, 1, 2);
  draft.accept(response(source, 0, 13), draft.generation);
  assert.deepEqual(draft.request().ranges, [{noteIndex:0,firstFrame:13,lastFrame:14}]);
  draft.setRange(0, 0, 0);
  assert.deepEqual(draft.request().ranges, [{noteIndex:0,firstFrame:12,lastFrame:12}]);
});

test('saved absolute ranges migrate without changing the allowed frames; new records stay relative', () => {
  const notes = [0,1,2].map(() => [{time:0.1}]);
  const old = {ranges:[[{noteIndex:0,firstFrame:9,lastFrame:14}],[],[]]};
  const expected = [[{noteIndex:0,firstOffset:-3,lastOffset:2}],[],[]];
  assert.deepEqual(restoreFrameRanges(old,notes), expected);
  assert.deepEqual(frameRangesToAbsolute(expected[0],notes[0]), old.ranges[0]);
  assert.deepEqual(restoreFrameRanges({coordinates:RANGE_COORDINATES,ranges:expected},notes), expected);
  assert.throws(() => restoreFrameRanges({coordinates:'future',ranges:expected},notes), /版本/);
  assert.throws(() => restoreFrameRanges({ranges:expected},notes), /旧版/);
  assert.throws(() => restoreFrameRanges({ranges:[old.ranges[0],[],[{noteIndex:9,firstFrame:0,lastFrame:0}]]},notes));
});

test('drafts preserve the source and its full precision power while changing only per-note limits', () => {
  const source = timingResult(), before = JSON.stringify(source);
  const draft = new TimingPlanDraft(source, 0);
  draft.setRange(0, 1, 2);
  const request = draft.request();
  assert.equal(request.input.power, 12345.125);
  assert.deepEqual(request.ranges, [{noteIndex:0,firstFrame:13,lastFrame:14}]);
  assert.equal(request.input, source.hitTiming.songs[0].input);
  assert.equal(draft.result, null);
  assert.equal(JSON.stringify(source), before);
  assert.throws(() => makeTimingPlanRequest(source, 3, []));
});

test('only an evaluation of the current range generation can update scores', () => {
  const source = timingResult(), draft = new TimingPlanDraft(source, 0);
  draft.setRanges([range()]);
  const stale = draft.generation;
  draft.setRanges([range(2, 2)]);
  assert.equal(draft.accept(response(source, 0, 13), stale), false);
  assert.equal(draft.result, null);
  assert.throws(() => draft.accept(response(source, 0, 13), draft.generation), /超出指定范围/);
  assert.equal(draft.accept(response(source, 0, 14), draft.generation), true);
  assert.equal(draft.result.total, 90);
  const corrupt = response(source, 0, 14); corrupt.result.trace[0].score++;
  assert.throws(() => draft.accept(corrupt, draft.generation), /不完整/);
});

test('undo, redo and reset restore range state without certifying stale computed results', () => {
  const source = timingResult(), draft = new TimingPlanDraft(source, 0);
  draft.setRanges([range()]); draft.accept(response(source, 0, 13), draft.generation);
  draft.setRanges([range(2, 2)]); assert.equal(draft.result, null);
  draft.undo(); assert.equal(draft.result.total, 90);
  draft.redo(); assert.equal(draft.result, null);
  draft.removeRange(0); assert.equal(draft.result, source.hitTiming.songs[0].result);
  draft.undo(); assert.deepEqual(draft.ranges, [range(2, 2)]);
  draft.setRanges([range(3, 3)]); assert.equal(draft.future.length, 0);
});

test('infeasible ranges retain editable state and export is gated on all three songs', () => {
  const source = timingResult(), drafts = [0, 1, 2].map(i => new TimingPlanDraft(source, i));
  drafts[0].setRanges([range()]);
  drafts[0].accept({planRevision: PLAN_REVISION, status: 'infeasible', message: '范围冲突', noteIndices: [0]}, drafts[0].generation);
  assert.equal(drafts[0].result, null);
  assert.deepEqual(drafts[0].ranges, [range()]);
  assert.throws(() => timingPlanExport(source, drafts), /先完成/);
  drafts[0].accept(response(source, 0, 13), drafts[0].generation);
  const exported = JSON.parse(JSON.stringify(timingPlanExport(source, drafts)));
  assert.equal(exported.totalScore, 290);
  assert.equal(exported.sourceTotalScore, 300);
  assert.equal(exported.scope, 'fixedTeamsAndSkillOrders');
  assert.equal(exported.hitTiming.probabilitySemantics, 'fixedChosenOrder');
  assert.equal(isCompleteTimingResult(exported), false);
  assert.equal(exported.schemaVersion, 2);
  assert.equal(exported.rangeCoordinates.relativeRanges, RANGE_COORDINATES);
  assert.deepEqual(exported.hitTiming.songs[0].relativeRanges, [range()]);
  assert.deepEqual(exported.hitTiming.songs[0].ranges, [{noteIndex:0,firstFrame:13,lastFrame:14}]);
  assert.deepEqual(exported.hitTiming.songs[0].input.hitFrames, [13]);
  assert.equal(source.totalScore, 300);
});

test('row numbering stays tied to original note indices after hit dispatch order changes', () => {
  const source = timingResult(), song = source.hitTiming.songs[0];
  song.input.notes.push({time: 0.11, trigger: 0});
  song.input.hitFrames.push(14);
  song.input.nativeTimeRouting = {targets: [{button: 3}, {button: 6}]};
  song.input.constraints = [{before: 0, after: 1}];
  const rows = planNoteRows(song, {hitFrames: [15, 14], trace: [{noteIndex: 1, score: 20}, {noteIndex: 0, score: 10}]});
  assert.deepEqual(rows.map(r => [r.number, r.lane, r.hit.score]), [[1, 4, 10], [2, 7, 20]]);
  assert.equal(rows[0].kind, '绿条头');
  assert.equal(rows[1].kind, '绿条尾');
});

test('displayed node types distinguish Long edges and all declared Slide roles', () => {
  const windows = [{kind:'standard'}, {kind:'widenedEndpoint'},
    ...['head','middle','end','flickEnd','directionalEnd'].map(role => ({kind:'slideTime',role})),
    {kind:'standard'}];
  const witness = {input:{
    notes:windows.map((window,i)=>({time:i+1,window})),
    hitFrames:windows.map((_,i)=>(i+1)*120),
    constraints:[{before:0,after:1},{before:2,after:3},{before:3,after:4}],
  }};
  const rows = planNoteRows(witness);
  assert.deepEqual(rows.map(r=>r.kind), ['长按头','长按尾','绿条头','绿条中间','绿条尾','绿条尾滑','绿条定向尾滑','单键']);
  assert.deepEqual(rows.map(r=>r.connectionKind), ['long','long','slide','slide','slide','slide','slide',null]);
  assert.deepEqual(rows.map(r=>r.chartFrame), witness.input.hitFrames);
});

test('one edit recomputes all three songs and publishes simplified empty-range results together', async () => {
  const source = timingResult(), drafts = [0,1,2].map(i => new TimingPlanDraft(source,i));
  drafts[0].setRange(0,1,2);
  const called = [], progress = [];
  const applied = await recomputeTimingPlans(drafts, async request => {
    const i = called.length; called.push(request);
    assert.equal(drafts[0].result, null, 'nothing is published before all three finish');
    return response(source,i,13,95);
  }, {onProgress:i => progress.push(i)});
  assert.equal(applied,true); assert.equal(called.length,3);
  assert.deepEqual(called.map(r => r.ranges.length),[1,0,0]);
  assert.deepEqual(progress,[0,1,2]);
  assert.deepEqual(drafts.map(d => d.result.hitFrames[0]),[13,13,13]);
  assert.equal(timingPlanExport(source,drafts).totalScore,285);
  assert.equal(source.totalScore,300);
});

test('three-song recompute never publishes a partial result after failure, corruption or a newer edit', async () => {
  for (const mode of ['error','corrupt','newer','disconnected']) {
    const source = timingResult(), drafts = [0,1,2].map(i => new TimingPlanDraft(source,i));
    drafts[0].setRange(0,1,2); let calls = 0, connected = true;
    const task = recomputeTimingPlans(drafts, async () => {
      const i = calls++;
      if (i === 1 && mode === 'error') throw new Error('failed');
      if (i === 1 && mode === 'newer') drafts[0].setRange(0,2,2);
      if (i === 1 && mode === 'disconnected') connected = false;
      const r = response(source,i,13);
      if (i === 2 && mode === 'corrupt') r.result.trace[0].score++;
      return r;
    }, {isCurrent:() => connected});
    if (['error','corrupt'].includes(mode)) await assert.rejects(task);
    else assert.equal(await task,false);
    assert.equal(drafts[0].result,null);
    assert.equal(drafts[1].result,source.hitTiming.songs[1].result);
    assert.equal(drafts[2].result,source.hitTiming.songs[2].result);
  }
});

test('an infeasible song still completes the other two and blocks the combined export', async () => {
  const source = timingResult(), drafts = [0,1,2].map(i => new TimingPlanDraft(source,i));
  drafts[0].setRange(0,1,2); let calls = 0;
  assert.equal(await recomputeTimingPlans(drafts, async () => {
    const i = calls++;
    return i === 0 ? {planRevision:PLAN_REVISION,status:'infeasible',message:'范围冲突',noteIndices:[0]} : response(source,i,12);
  }),true);
  assert.equal(calls,3); assert.equal(drafts[0].result,null);
  assert.deepEqual(drafts.slice(1).map(d => d.result.total),[90,90]);
  assert.throws(() => timingPlanExport(source,drafts), /先完成/);
});
