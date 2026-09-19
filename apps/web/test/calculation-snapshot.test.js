import test from 'node:test';
import assert from 'node:assert/strict';
import {createCalculationActions} from '../src/actions/calculation.js';

function gate(){let resolve;const promise=new Promise(r=>resolve=r);return {promise,resolve};}
function harness({ensureCore=async()=>({}),calculate=async()=>({totalScore:42}),getActivePage=()=>undefined,hasOpenDialog=()=>false,calculateButtons=[],initialPlayer={},eventRecord={eventType:'versus'}}={}){
  let player={currentEvent:0,server:'cn',calculationMode:'maximize',activityMode:'single',cardList:{1:{skillLevel:5,illustTrainingStatus:true}},...initialPlayer};
  const state={activePlayerProfileId:'A',runtime:{calculate,ptMaximize:calculate},resultCache:[]};
  const writes=[],renders=[],errors=[],navigations=[];
  const field={value:'',checked:false,disabled:false,required:false,querySelector:()=>null,querySelectorAll:()=>[]};
  const elements=new Proxy({eventId:{value:String(player.currentEvent)},scoreRangeAutoBaseMultiplier:{value:'0.5'},result:{},calculateButtons},{get:(o,k)=>k in o?o[k]:k==='calculateButton'?null:field});
  const noop=()=>{};
  const actions=createCalculationActions({state,elements,readPlayer:()=>structuredClone(player),writePlayer:p=>{writes.push(p);player=structuredClone(p);},savePlayerNow:async()=>{},readOptionalInteger:v=>v===''?undefined:Number(v),parseEntityId:v=>Number(v),applyEventInputToPlayer:noop,editableEventSnapshot:()=>eventRecord,normalizeCurrentActivityForMode:noop,ensureOwnedCardCharacterBonuses:noop,ensureCore,renderConfigForms:noop,renderResultSummary:r=>renders.push(r),renderMetrics:noop,buildDiagnostic:async data=>data,activatePage:p=>navigations.push(p),getActivePage,hasOpenDialog,setStatus:noop,setError:e=>errors.push(e),eventLabel:()=> '活动',normalizedPlayer:p=>p,persistResultCache:async()=>{},yieldForPaint:async()=>{}});
  return {state,elements,actions,writes,renders,errors,navigations,getPlayer:()=>player,setScope:value=>{player.eventAvailableCardsOnly=value;},edit:()=>{player.cardList[1].skillLevel=2;}};
}

test('festival submits only rank and victory; obsolete history stays viewable but is not reused',async()=>{
 for(const won of [false,true]){
  const calls=[],h=harness({initialPlayer:{calculationMode:'ptMaximize',ptMaximize:{liveVariantByEventType:{festival:'festival'},festivalTeammateScores:[null,-1,99999999,0]}},eventRecord:{eventType:'festival'},calculate:async input=>{calls.push(input);return {liveVariant:'festival',totalScore:calls.length,scenario:{festival:input.request.festival}};}});
  h.elements.ptMaximizeFestivalRank={querySelector:()=>({value:'3'})};
  h.elements.ptMaximizeFestivalWon={querySelector:()=>({value:String(won)})};
  await h.actions.handleCalculate({preventDefault(){}});
  assert.deepEqual(h.errors,[]);
  assert.deepEqual(calls[0].request.festival,{teamRank:3,won});
  assert.equal('festivalTeammateScores' in calls[0].player.ptMaximize,false);
  const entry=h.state.resultCache[0];
  entry.result.scenario.festival.teammateScores=4000000;
  entry.diagnostic.calculationRequest.festival.teammateScores=4000000;
  const key=entry.key;
  await h.actions.handleResultCacheAction({preventDefault(){},target:{closest:()=>({dataset:{resultCacheAction:'restore',resultCacheKey:key}})}});
  assert.equal(h.state.lastDiagnostic.result.totalScore,1);
  await h.actions.handleCalculate({preventDefault(){}});
  assert.equal(calls.length,2);
  assert.equal(h.state.lastDiagnostic.result.totalScore,2);
  await h.actions.handleCalculate({preventDefault(){}});
  assert.equal(calls.length,2);
  assert.deepEqual(h.errors,[]);
 }
});

test('automatic runtimes receive the filtered snapshot; switching the restriction bypasses the other cache',async()=>{
 for(const calculationMode of ['maximize','ptMaximize']){
  const calls=[],eventRecord={eventType:'versus',endAt:[null,null,null,200,null]},h=harness({initialPlayer:{currentEvent:1,calculationMode,eventAvailableCardsOnly:true,cardList:{1:{skillLevel:5},2:{skillLevel:5}}},eventRecord,ensureCore:async()=>({cards:{1:{releasedAt:[null,null,null,100,null]},2:{releasedAt:[null,null,null,300,null]}}}),calculate:async input=>{calls.push(input);return {totalScore:calls.length};}});
  await h.actions.handleCalculate({preventDefault(){}});
  assert.deepEqual(h.errors,[]);
  assert.deepEqual(Object.keys(calls[0].player.cardList),['1']);
  assert.deepEqual(Object.keys(h.getPlayer().cardList),['1','2']);
  assert.deepEqual(Object.keys(h.state.lastDiagnostic.player.cardList),['1']);
  assert.deepEqual(Object.keys(h.state.lastDiagnostic.sourcePlayer.cardList),['1','2']);
  h.setScope(false);await h.actions.handleCalculate({preventDefault(){}});
  assert.equal(calls.length,2);assert.deepEqual(Object.keys(calls[1].player.cardList),['1','2']);
  h.setScope(true);await h.actions.handleCalculate({preventDefault(){}});
  assert.deepEqual(h.errors,[]);assert.equal(calls.length,2);assert.equal(h.state.lastDiagnostic.result.totalScore,1);
  eventRecord.endAt[3]=null;await h.actions.handleCalculate({preventDefault(){}});
  assert.deepEqual(h.errors,[]);assert.equal(calls.length,3);
  assert.deepEqual(Object.keys(calls[2].player.cardList),['1','2']);
  assert.equal(h.state.lastDiagnostic.cardAvailability.fallback,'missing-event-end');
  await h.actions.handleCalculate({preventDefault(){}});assert.equal(calls.length,3);
  eventRecord.endAt[3]=200;await h.actions.handleCalculate({preventDefault(){}});
  assert.deepEqual(h.errors,[]);assert.equal(calls.length,4);
  assert.deepEqual(Object.keys(calls[3].player.cardList),['1']);
 }
});
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

test('invalidated history remains viewable but a new calculation bypasses its score',async()=>{
 let calls=0;
 const h=harness({calculate:async()=>({totalScore:++calls})});
 await h.actions.handleCalculate({preventDefault(){}});
 const saved=structuredClone(h.state.resultCache[0]);
 await h.actions.handleCalculate({preventDefault(){}});
 assert.equal(calls,1);
 await h.actions.invalidateResultCache();
 assert.equal(h.state.resultCache.length,1);
 assert.equal(h.state.resultCache[0].reusable,false);
 assert.deepEqual(h.state.resultCache[0].result,saved.result);
 assert.deepEqual(h.state.resultCache[0].diagnostic,saved.diagnostic);
 await h.actions.handleResultCacheAction({preventDefault(){},target:{closest:()=>({dataset:{resultCacheAction:'restore',resultCacheKey:saved.key}})}});
 assert.equal(h.state.lastDiagnostic.result.totalScore,1);
 await h.actions.handleCalculate({preventDefault(){}});
 assert.equal(calls,2);
 assert.equal(h.state.resultCache[0].reusable,true);
 assert.deepEqual(h.errors,[]);
});

test('a calculation started before resource invalidation cannot repopulate reusable cache',async()=>{
 const ready=gate(),entered=gate();let calls=0;
 const h=harness({calculate:async()=>{calls++;entered.resolve();await ready.promise;return {totalScore:calls};}});
 const run=h.actions.handleCalculate({preventDefault(){}});
 await entered.promise;
 assert.equal(h.actions.hasActiveCalculation(),true);
 await h.actions.invalidateResultCache();
 ready.resolve();await run;
 assert.equal(h.actions.hasActiveCalculation(),false);
 assert.equal(h.state.resultCache[0].reusable,false);
 await h.actions.handleCalculate({preventDefault(){}});
 assert.equal(calls,2);
 assert.equal(h.state.resultCache[0].reusable,true);
 assert.deepEqual(h.errors,[]);
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
