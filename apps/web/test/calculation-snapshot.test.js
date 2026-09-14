import test from 'node:test';
import assert from 'node:assert/strict';
import {createCalculationActions} from '../src/actions/calculation.js';

function gate(){let resolve;const promise=new Promise(r=>resolve=r);return {promise,resolve};}
function harness({ensureCore=async()=>({}),calculate=async()=>({totalScore:42}),getActivePage=()=>undefined,hasOpenDialog=()=>false,calculateButtons=[]}={}){
  let player={currentEvent:0,server:'cn',calculationMode:'maximize',activityMode:'single',cardList:{1:{skillLevel:5,illustTrainingStatus:true}}};
  const state={activePlayerProfileId:'A',runtime:{calculate},resultCache:[]};
  const writes=[],renders=[],errors=[],navigations=[];
  const field={value:'',checked:false,disabled:false,required:false,querySelector:()=>null,querySelectorAll:()=>[]};
  const elements=new Proxy({eventId:{value:'0'},scoreRangeAutoBaseMultiplier:{value:'0.5'},result:{},calculateButtons},{get:(o,k)=>k in o?o[k]:k==='calculateButton'?null:field});
  const noop=()=>{};
  const actions=createCalculationActions({state,elements,readPlayer:()=>structuredClone(player),writePlayer:p=>{writes.push(p);player=structuredClone(p);},savePlayerNow:async()=>{},readOptionalInteger:v=>v===''?undefined:Number(v),parseEntityId:v=>Number(v),applyEventInputToPlayer:noop,editableEventSnapshot:()=>({eventType:'versus'}),normalizeCurrentActivityForMode:noop,ensureOwnedCardCharacterBonuses:noop,ensureCore,renderConfigForms:noop,renderResultSummary:r=>renders.push(r),renderMetrics:noop,buildDiagnostic:async data=>data,activatePage:p=>navigations.push(p),getActivePage,hasOpenDialog,setStatus:noop,setError:e=>errors.push(e),eventLabel:()=> '活动',normalizedPlayer:p=>p,persistResultCache:async()=>{},yieldForPaint:async()=>{}});
  return {state,actions,writes,renders,errors,navigations,getPlayer:()=>player,edit:()=>{player.cardList[1].skillLevel=2;}};
}
test('edits made during data synchronization survive; calculation keeps its submitted snapshot',async()=>{
  const ready=gate(),entered=gate();let submitted;
  const h=harness({ensureCore:async()=>{entered.resolve();return ready.promise;},calculate:async({player})=>{submitted=player;return {totalScore:42};}});
  const run=h.actions.handleCalculate({preventDefault(){}});await Promise.race([entered.promise,run.then(()=>{throw h.errors[0]||new Error("calculation did not reach runtime");})]);h.edit();ready.resolve({});await run;
  assert.deepEqual(h.errors,[]);assert.equal(h.getPlayer().cardList[1].skillLevel,2);assert.equal(h.writes.length,0);assert.equal(submitted.cardList[1].skillLevel,5);
});
test('late calculation returns to its originating history without replacing another profile screen',async()=>{
  const ready=gate(),entered=gate();
  const h=harness({calculate:async()=>{entered.resolve();return ready.promise;}});
  const run=h.actions.handleCalculate({preventDefault(){}});await Promise.race([entered.promise,run.then(()=>{throw h.errors[0]||new Error("calculation did not reach runtime");})]);h.state.activePlayerProfileId='B';ready.resolve({totalScore:42});await run;
  assert.deepEqual(h.errors,[]);assert.equal(h.renders.length,0);assert.equal(h.state.resultCache[0].profileId,'A');assert.equal(h.state.profileDiagnostics.A.result.totalScore,42);assert.equal(h.state.lastDiagnostic,undefined);
});

test('a completed calculation saves the result without interrupting navigation or an open editor',async()=>{
 for(const modal of [false,true]){
  let page='#activity';const ready=gate(),entered=gate();
  const h=harness({getActivePage:()=>page,hasOpenDialog:()=>modal,calculate:async()=>{entered.resolve();return ready.promise;}});
  const run=h.actions.handleCalculate({preventDefault(){}});await entered.promise;if(!modal)page='#archive';ready.resolve({totalScore:42});await run;
  assert.deepEqual(h.errors,[]);assert.equal(h.renders.length,1);assert.deepEqual(h.navigations,[]);assert.equal(h.state.profileDiagnostics.A.result.totalScore,42);
 }
});
test('main band import validates player ID before requesting account data',async()=>{
 const h=harness();let called=false;h.state.runtime.importBestdoriPlayerProfile=async()=>{called=true;};
 await assert.rejects(h.actions.loadMainBandDraft({playerId:0},0),/填写玩家 ID/);assert.equal(called,false);
});

test('every calculate entry stays busy and restores its own label after success or failure',async()=>{
 for(const fails of [false,true]){
  const buttons=['计算','重新计算','重新计算'].map(textContent=>({textContent,disabled:false,querySelector:()=>null,classList:{toggle(){}},setAttribute(name,value){this[name]=value;}}));
  const entered=gate(),ready=gate();let calls=0;
  const h=harness({calculateButtons:buttons,calculate:async()=>{calls++;entered.resolve();await ready.promise;if(fails)throw new Error('test failure');return {totalScore:42};}});
  const run=h.actions.handleCalculate({preventDefault(){}});await entered.promise;
  assert.ok(buttons.every(b=>b.disabled&&b.textContent==='计算中'&&b['aria-busy']==='true'));
  await h.actions.handleCalculate({preventDefault(){}});assert.equal(calls,1);
  ready.resolve();await run;
  assert.deepEqual(buttons.map(b=>b.textContent),['计算','重新计算','重新计算']);
  assert.ok(buttons.every(b=>!b.disabled&&b['aria-busy']==='false'));
  assert.deepEqual(h.errors,[]);
  if(fails)assert.equal(h.state.lastDiagnostic.error.message,'test failure');
  else assert.equal(h.state.lastDiagnostic.result.totalScore,42);
 }
});
