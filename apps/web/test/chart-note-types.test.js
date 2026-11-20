import assert from 'node:assert/strict';
import test from 'node:test';
import {timingChartNoteTypes} from '../src/domain/chart-note-types.js';
import {planNoteRows} from '../src/domain/timing-plan.js';

function fixture() {
  const raw=[{type:'BPM',beat:0,bpm:120},
    {type:'Single',beat:8,lane:0,flick:true},
    {type:'Directional',beat:2,lane:0,direction:'Left',width:3},
    {type:'Slide',connections:[{beat:1,lane:1},{beat:1.5,lane:1,hidden:true},{beat:2,lane:2,flick:true}]},
    {type:'Long',connections:[{beat:3,lane:4},{beat:4,lane:4,flick:true}]},
    {type:'BPM',beat:4,bpm:60},{type:'Single',beat:5,lane:6,skill:true}];
  const windows=[{kind:'slideTime',role:'head'},{kind:'standard'},{kind:'slideTime',role:'flickEnd'},
    {kind:'standard'},{kind:'widenedEndpoint'},{kind:'standard'},{kind:'standard'}];
  const input={notes:[.5,1,1,1.5,2,3,6].map((time,i)=>({time,window:windows[i],trigger:i===5?0:null})),
    nativeTimeRouting:{targets:[1,0,2,4,4,6,0].map(button=>({button}))},
    constraints:[{before:0,after:2},{before:3,after:4}],hitFrames:[60,120,120,180,240,360,720]};
  return {raw,witness:{input,result:{hitFrames:[59,120,120,179,240,360,721],trace:[]}}};
}

test('source types follow visible beat/event/node order across BPM changes and keep witness coordinates',()=>{
  const {raw,witness}=fixture(),before=JSON.stringify(witness);
  const types=timingChartNoteTypes(witness,raw),rows=planNoteRows(witness,witness.result,types);
  assert.deepEqual(types.map(t=>t.visualType),['hold','directional','flick','hold','flick','tap','flick']);
  assert.equal(types[1].direction,'Left');assert.equal(types[1].width,3);
  assert.deepEqual(rows.map(r=>r.number),[1,2,3,4,5,6,7]);
  assert.deepEqual(rows.map(r=>r.frame),witness.result.hitFrames);
  assert.deepEqual(rows.map(r=>r.chartFrame),witness.input.hitFrames);
  assert.deepEqual(rows.map(r=>r.connectionKind),['slide',null,'slide','long','long',null,null]);
  assert.equal(JSON.stringify(witness),before);
});

test('mismatched chart geometry, routes, trigger identity or chain never supplies guessed types',()=>{
  for(const change of [
    raw=>raw[1].beat++,raw=>raw[2].lane++,raw=>raw[3].connections[1].hidden=false,
    raw=>raw[3].connections[2].flick=false,raw=>raw[6].skill=false,raw=>raw[5].bpm=90,
    (raw,witness)=>witness.input.constraints.pop(),raw=>raw.pop(),raw=>raw[2].width=99,
    raw=>raw[2].direction='Up',
  ]) {
    const {raw,witness}=fixture();change(raw,witness);
    assert.throws(()=>timingChartNoteTypes(witness,raw),/不一致/);
  }
});
