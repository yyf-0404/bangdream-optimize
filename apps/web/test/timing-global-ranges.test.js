import assert from 'node:assert/strict';
import test from 'node:test';
import {TimingPlanDraft,recomputeGlobalTimingPlans,referenceFrameRanges,GLOBAL_RANGE_REVISION} from '../src/domain/timing-plan.js';
import {isCompleteTimingResult} from '../src/domain/hit-timing.js';
import {resultCacheInvalidationReason} from '../src/data/result-cache.js';
import {timingResult} from './fixtures/timing-result.js';

function response(source,request) {
  const next=structuredClone(source);
  next.hitTiming.rangeRestrictions=structuredClone(request);
  next.songs.forEach((s,i)=>{s.teamCardIds=[100+i,200+i,300+i,400+i,500+i];});
  next.hitTiming.songs.forEach(s=>{s.input.power=45678.25;s.result.scoringPower=45678.25;});
  return next;
}

test('one global request can replace all teams, orders and powers and publishes three songs together',async()=>{
  const source=timingResult(),before=JSON.stringify(source),drafts=[0,1,2].map(i=>new TimingPlanDraft(source,i,{includeReference:true}));
  drafts[0].setRange(0,-2,0);drafts[2].setRange(0,0,3);
  let calls=0;
  const next=await recomputeGlobalTimingPlans(drafts,async request=>{
    calls++;
    assert.equal(request.revision,GLOBAL_RANGE_REVISION);
    assert.deepEqual(request.songs.map(s=>s.ranges.length),[1,0,1]);
    assert.equal(drafts[0].result,null);
    return response(source,request);
  });
  assert.equal(calls,1);
  assert.equal(drafts[0].original.input.power,45678.25);
  assert.equal(drafts[2].evaluatedResult,next.hitTiming.songs[2].result);
  assert.deepEqual(next.songs[0].teamCardIds,[100,200,300,400,500]);
  assert.equal(JSON.stringify(source),before);
  assert.match(resultCacheInvalidationReason({result:next}),/自定义/);
});

test('single-song limits keep one chart and reject another event response', async () => {
  const source = timingResult(300, 'challenge');
  const draft = new TimingPlanDraft(source, 0, { includeReference: true });
  draft.setRange(0, 0, 0);
  await assert.rejects(recomputeGlobalTimingPlans([draft], async request => {
    const next = response(source, request);
    next.eventType = 'versus';
    return next;
  }), /当前活动/);
  assert.equal(draft.result, null);
  const next = await recomputeGlobalTimingPlans([draft], async request => {
    assert.equal(request.songs.length, 1);
    assert.deepEqual(request.songs[0].ranges, [{ noteIndex: 0, firstOffset: 0, lastOffset: 0 }]);
    return response(source, request);
  });
  assert.deepEqual(draft.result, next.hitTiming.songs[0].result);
});

test('bad last-song binding or witness never commits either earlier song',async()=>{
  for (const corrupt of [
    next=>next.hitTiming.rangeRestrictions.songs[2].ranges.push({noteIndex:0,firstOffset:-1,lastOffset:1}),
    next=>next.hitTiming.songs[2].result.trace[0].score++,
    next=>next.hitTiming.songs[2].input.notes[0].time=0.2,
  ]) {
    const source=timingResult(),drafts=[0,1,2].map(i=>new TimingPlanDraft(source,i));
    drafts[0].setRange(0,-1,0);
    await assert.rejects(recomputeGlobalTimingPlans(drafts,async r=>{
      const next=response(source,r);corrupt(next);return next;
    }));
    assert.equal(drafts[0].evaluatedResult,null);
    assert.equal(drafts[1].original,source.hitTiming.songs[1]);
  }
});

test('changed ranges or a closed/switched view discard completed global responses',async()=>{
  const source=timingResult(),drafts=[0,1,2].map(i=>new TimingPlanDraft(source,i));
  drafts[0].setRange(0,-1,0);
  assert.equal(await recomputeGlobalTimingPlans(drafts,async r=>{
    drafts[2].setRange(0,0,0);return response(source,r);
  }),null);
  assert.equal(drafts[0].evaluatedResult,null);
  let active=true;
  assert.equal(await recomputeGlobalTimingPlans(drafts,async r=>{active=false;return response(source,r);},{isCurrent:()=>active}),null);
});

test('original frame is always allowed; stale charts and altered transported ranges are rejected',async()=>{
  assert.throws(()=>referenceFrameRanges([{noteIndex:0,firstOffset:1,lastOffset:2}],1),/原始帧/);
  const source=timingResult(),drafts=[0,1,2].map(i=>new TimingPlanDraft(source,i,{includeReference:true}));
  assert.throws(()=>drafts[0].setRange(0,-2,-1),/原始帧/);
  drafts[0].setRange(0,0,0);
  await assert.rejects(recomputeGlobalTimingPlans(drafts,async r=>{
    const next=response(source,r);next.hitTiming.songs[0].input.initialCombo=10;return next;
  }));
  const next=response(source,{revision:GLOBAL_RANGE_REVISION,songs:source.hitTiming.songs.map(s=>({
    songId:s.songId,difficulty:s.difficulty,reference:s.input,ranges:[],
  }))});
  assert.equal(isCompleteTimingResult(next),true);
  next.hitTiming.rangeRestrictions.songs[0].ranges=[{noteIndex:0,firstOffset:1,lastOffset:3}];
  assert.equal(isCompleteTimingResult(next),false);
});
