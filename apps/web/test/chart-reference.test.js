import assert from 'node:assert/strict';
import test from 'node:test';
import {readFileSync} from 'node:fs';
import {chartReferenceFrame} from '../src/domain/chart-reference.js';
import {TimingPlanDraft, frameRangesToAbsolute, planNoteRows, restoreFrameRanges,
  recomputeGlobalTimingPlans, GLOBAL_RANGE_REVISION} from '../src/domain/timing-plan.js';
import {isCompleteTimingResult} from '../src/domain/hit-timing.js';
import {timingResult} from './fixtures/timing-result.js';

const cases=JSON.parse(readFileSync(new URL('../../../crates/core/tests/fixtures/chart-reference-frames.json',import.meta.url),'utf8'));
const offsetCases=JSON.parse(readFileSync(new URL('../../../crates/core/tests/fixtures/reference-offset-windows.json',import.meta.url),'utf8'));

test('every supported offset transports to the native absolute frame without recentering grades',()=>{
  for (const c of offsetCases) {
    assert.equal(chartReferenceFrame(c.note,c.target),c.reference,c.name);
    // The same hand-derived fixture is checked against all native raw grades.
    // Here each absolute-frame probe verifies the browser's coordinate adapter.
    for (let frame=0;frame<=72000;frame++) {
      const offset=frame-c.reference;
      const [range]=frameRangesToAbsolute([{noteIndex:0,firstOffset:Math.min(0,offset),lastOffset:Math.max(0,offset)}],
        [c.note],[c.target]);
      assert.equal(range.firstFrame,Math.min(c.reference,frame),c.name);
      assert.equal(range.lastFrame,Math.max(c.reference,frame),c.name);
    }
  }
});
test('chart references agree with native typed-window and lifetime fixtures',()=>{
  for (const c of cases) {
    if (c.expected===null) assert.throws(()=>chartReferenceFrame(c.note,c.target),undefined,c.name);
    else assert.equal(chartReferenceFrame(c.note,c.target),c.expected,c.name);
  }
});

test('zero offset pins fractional flick to arrival and is not shown as a moved note',async()=>{
  const source=timingResult(),song=source.hitTiming.songs[0];
  song.input.notes[0]={time:2.001,window:{kind:'slideTime',role:'flickEnd',last_frame:254}};
  song.input.hitFrames[0]=song.result.hitFrames[0]=241;
  Object.assign(song.result.trace[0],{frame:241,hitTime:241/120,offsetMs:(241/120-2.001)*1000});
  const drafts=[0,1,2].map(i=>new TimingPlanDraft(source,i,{includeReference:true}));
  drafts[0].setRange(0,0,0);
  assert.deepEqual(drafts[0].request().ranges,[{noteIndex:0,firstFrame:241,lastFrame:241}]);
  assert.equal(planNoteRows(song)[0].chartFrame,planNoteRows(song)[0].frame);
  const next=await recomputeGlobalTimingPlans(drafts,async request=>{
    assert.equal(request.revision,GLOBAL_RANGE_REVISION);
    const result=structuredClone(source);result.hitTiming.rangeRestrictions=request;return result;
  });
  assert.equal(isCompleteTimingResult(next),true);
  const early=structuredClone(next),bad=early.hitTiming.songs[0];
  bad.input.hitFrames[0]=bad.result.hitFrames[0]=bad.result.trace[0].frame=240;
  assert.equal(isCompleteTimingResult(early),false,'old rounded frame violates the new zero-offset constraint');
  next.hitTiming.rangeRestrictions.revision='global-relative-ranges-120-v1';
  assert.equal(isCompleteTimingResult(next),false);
});

test('midpoint lifetime also anchors limits, rows and legacy absolute imports',()=>{
  const note={time:2.001,window:{kind:'slideTime',role:'middle',last_frame:267}};
  const target={button:0,absolutePos:0,activeFromFrame:239,activeUntilFrame:239};
  assert.deepEqual(frameRangesToAbsolute([{noteIndex:0,firstOffset:0,lastOffset:0}],[note],[target]),
    [{noteIndex:0,firstFrame:239,lastFrame:239}]);
  const saved={ranges:[[{noteIndex:0,firstFrame:239,lastFrame:239}],[],[]]};
  assert.deepEqual(restoreFrameRanges(saved,[[note],[],[]],[[target],[],[]]),
    [[{noteIndex:0,firstOffset:0,lastOffset:0}],[],[]]);
  assert.throws(()=>restoreFrameRanges({coordinates:'chartFrameOffset120-v1',ranges:[[],[],[]]},[[note],[],[]]),/版本/);
});
