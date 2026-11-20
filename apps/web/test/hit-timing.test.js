import assert from 'node:assert/strict';
import test from 'node:test';
import {timingOptimizationEnabled,timingWitnessExport,timingSongSummary,isCompleteTimingResult,requireCompleteTimingResult} from '../src/domain/hit-timing.js';
import {resultCacheInvalidationReason} from '../src/data/result-cache.js';
import {timingResult as result} from './fixtures/timing-result.js';

test('the explicit preference enables timing for highest score', () => {
  for (const calculationMode of ['maximize', 'ptMaximize', 'ptEvaluate', 'scoreRange']) {
    assert.equal(timingOptimizationEnabled({ calculationMode, hitTiming: true }), calculationMode === 'maximize');
  }
  assert.equal(timingOptimizationEnabled({ calculationMode: 'maximize' }), false);
});

test('each supported highest-score activity exports its own song count and combo rule', () => {
  for (const eventType of ['challenge', 'versus', 'medley']) {
    const source = result(300, eventType);
    assert.equal(isCompleteTimingResult(source), true);
    assert.deepEqual(timingWitnessExport(source).teams, source.songs);
    source.hitTiming.songs[0].input.medleyCombo = eventType !== 'medley';
    assert.equal(isCompleteTimingResult(source), false);
  }
});

test('obsolete or incomplete timing results remain identifiable and cannot be reused',()=>{
  const entry={result:result()};
  assert.equal(resultCacheInvalidationReason(entry),'');
  for(const [field,value] of [['complete',false],['timingRevision','old'],['timingRevision','typed-time-windows-v12'],['timingRevision','typed-time-windows-v11'],['timingRevision','projected-green-constraints-v9'],['preparationRevision','old'],['preparationRevision','visible-typed-time-midpoint-120-v5'],['preparationRevision','visible-typed-time-midpoint-120-v4'],['preparationRevision','visible-time-midpoint-same-frame-120-v3'],['preparationRevision','visible-time-midpoint-120-v2'],['fps',60],['offsetFrames',1],['model','position']]){
    const old=structuredClone(entry);old.result.hitTiming[field]=value;
    assert.match(resultCacheInvalidationReason(old),/击打记录不完整或模型不兼容/);
    assert.equal(old.result.totalScore,300);
  }
});

test('witness JSON keeps frame ordering, scoring power and boundary time exactly',()=>{
  const source=result(),exported=JSON.parse(JSON.stringify(timingWitnessExport(source)));
  assert.deepEqual(exported.hitTiming,source.hitTiming);
  assert.deepEqual(exported.teams,source.songs);
  assert.equal(exported.schemaVersion,1);
  assert.throws(()=>timingWitnessExport({...source,hitTiming:{...source.hitTiming,complete:false}}),/没有完整/);
  assert.throws(()=>timingWitnessExport({...source,hitTiming:{...source.hitTiming,songs:[]}}),/没有完整/);
});

test('previous growth and card-source results stay exportable but cannot masquerade as fresh calculations',()=>{
  for(const revision of ['cn-9.4.4-f32-v1','cn-9.4.4-f32-v2-decimal-growth','cn-9.4.4-f32-v3-cn-card-source']){
    const old=result();old.scoringRevision=revision;
    for (const song of old.hitTiming.songs) song.result.scoringRevision=revision;
    assert.throws(()=>requireCompleteTimingResult(old),/不兼容的模型版本/);
    assert.match(resultCacheInvalidationReason({result:old}),/击打记录不完整或模型不兼容/);
    const exported=JSON.parse(JSON.stringify(timingWitnessExport(old)));
    assert.equal(exported.scoringRevision,old.scoringRevision);
    assert.deepEqual(exported.hitTiming,old.hitTiming);
  }
  const earlier=result();earlier.scoringRevision='cn-9.4.4-f32-v1';
  delete earlier.hitTiming.songs[0].input.hitOrder;
  delete earlier.hitTiming.songs[0].result.hitOrder;
  assert.equal(isCompleteTimingResult(earlier),false);
  assert.deepEqual(timingWitnessExport(earlier).hitTiming,earlier.hitTiming);
});

test('truncated or inconsistent witnesses are rejected before reuse or export',()=>{
  for (const mutate of [
    r=>{delete r.hitTiming.songs[0].input.hitOrder;delete r.hitTiming.songs[0].result.hitOrder;},
    r=>{r.hitTiming.songs[0].input.hitOrder=[-1];},
    r=>{r.hitTiming.songs[0].result.hitOrder=[1];},
    r=>{delete r.hitTiming.songs[0].result.trace;},
    r=>{r.hitTiming.songs[0].result.trace=[];},
    r=>{r.hitTiming.songs[0].result.trace[0].frame++;},
    r=>{r.hitTiming.songs[0].result.trace[0].score++;},
    r=>{r.hitTiming.songs[0].result.trace[0].judgement='bad';},
    r=>{r.hitTiming.songs[0].result.hitFrames[0]++;},
    r=>{r.hitTiming.songs[0].input.hitFrames[0]=0.5;},
    r=>{r.hitTiming.songs[0].input.hitFrames[0]=72_001;},
    r=>{r.songs[0].score++;},
    r=>{r.totalScore++;},
    r=>{r.hitTiming.songs[0].songId++;},
    r=>{r.hitTiming.songs[0].difficulty++;},
    r=>{r.hitTiming.songs[0].result.scoringRevision='obsolete';},
    r=>{r.hitTiming.songs[0].result.timingRevision='obsolete';},
    r=>{r.hitTiming.songs[0].result.fps=60;},
    r=>{r.hitTiming.songs[0].result.scoringPower++;},
    r=>{r.hitTiming.songs[1].input.initialCombo=0;},
    r=>{r.hitTiming.songs[0].input.medleyCombo=false;},
  ]) {
    const changed=result();mutate(changed);
    assert.equal(isCompleteTimingResult(changed),false);
    assert.throws(()=>requireCompleteTimingResult(changed),/完整见证/);
    assert.match(resultCacheInvalidationReason({result:changed}),/击打记录不完整或模型不兼容/);
    assert.throws(()=>timingWitnessExport(changed),/没有完整/);
  }
});

test('null and sparse JSON-shaped witness fields return false without breaking history loading',()=>{
  for (const mutate of [
    r=>{r.hitTiming.songs[0]=null;},
    r=>{r.songs[0]=null;},
    r=>{r.hitTiming.songs=null;},
    r=>{r.songs={length:3};},
    r=>{r.hitTiming.songs[0].input.notes[0]=null;},
    r=>{r.hitTiming.songs[0].result.trace[0]=null;},
    r=>{delete r.hitTiming.songs[0];},
    r=>{delete r.hitTiming.songs[0].input.notes[0];},
  ]) {
    const changed=result();mutate(changed);
    assert.equal(isCompleteTimingResult(changed),false);
    assert.match(resultCacheInvalidationReason({result:changed}),/击打记录不完整或模型不兼容/);
  }
});

test('explicit non-index order within a frame is retained and must be a complete permutation',()=>{
  const source=result();
  for (let i=0;i<3;i++) {
    const song=source.hitTiming.songs[i];
    song.input.notes.push(structuredClone(song.input.notes[0]));
    song.input.hitFrames=[12,12];song.result.hitFrames=[12,12];
    song.input.hitOrder=[1,0];song.result.hitOrder=[1,0];
    song.input.initialCombo=2*i;
    const first={...song.result.trace[0],noteIndex:1,combo:2*i+1,score:40};
    const second={...song.result.trace[0],noteIndex:0,combo:2*i+2,score:60};
    song.result.trace=[first,second];
  }
  assert.equal(isCompleteTimingResult(source),true);
  assert.deepEqual(timingWitnessExport(source).hitTiming.songs[0].input.hitOrder,[1,0]);
  for(const order of [[0,0],[0,2],[0,0.5],[]]) {
    const changed=structuredClone(source);
    changed.hitTiming.songs[0].input.hitOrder=order;
    changed.hitTiming.songs[0].result.hitOrder=order;
    assert.equal(isCompleteTimingResult(changed),false);
  }
});

test('display summaries use corrected judgments and real hit offsets',()=>{
  assert.deepEqual(timingSongSummary(result().hitTiming.songs[0]),{noteCount:1,perfect:1,great:0,minOffsetMs:1e-14,maxOffsetMs:1e-14});
  for(const song of [null,{}, {result:{trace:{}}}, {result:{trace:[null]}}]) {
    assert.deepEqual(timingSongSummary(song),{noteCount:0,perfect:0,great:0,minOffsetMs:0,maxOffsetMs:0});
  }
  assert.deepEqual(timingSongSummary({result:{trace:[{judgement:'great',offsetMs:null},{judgement:'perfect',offsetMs:Infinity}]}}),
    {noteCount:2,perfect:1,great:1,minOffsetMs:0,maxOffsetMs:0});
});
