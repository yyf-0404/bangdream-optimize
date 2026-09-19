import test from 'node:test';
import assert from 'node:assert/strict';
import {createResourceActions} from '../src/actions/resource.js';
import {createCalculationActions} from '../src/actions/calculation.js';
import {createResultCacheStorage} from '../src/data/result-cache.js';

const entries = () => ['A','B'].map((profileId,index)=>({
  cacheVersion:8,profileId,key:`saved-${index}`,createdAt:index+1,
  result:{totalScore:100+index},diagnostic:{player:{currentEvent:323},result:{totalScore:100+index}},
}));

function setup(t,{persistFails=false,clearProfilesFails=false,busy=false,confirm=true}={}) {
  const previousWindow=globalThis.window;
  const confirmations=[];
  globalThis.window={confirm:text=>{confirmations.push(text);return confirm;}};
  t.after(()=>{if(previousWindow===undefined)delete globalThis.window;else globalThis.window=previousWindow;});
  const steps=[],errors=[],renders=[];
  const state={activePlayerProfileId:'A',core:{old:true},resultCache:entries(),lastDiagnostic:{saved:true},profileDiagnostics:{A:{saved:true}}};
  const player={cardList:{1:{skillLevel:5}}},elements={calculateButton:null,result:{textContent:'saved'},log:{}};
  let persisted=entries();
  state.runtime={
    clearGameCache:async()=>steps.push('game'),
    refreshCoreGameData:async()=>steps.push('core'),
    syncAllGameData:async()=>steps.push('all'),
    clearLocalCache:async()=>{steps.push('profiles');if(clearProfilesFails)throw Error('profile write failed');},
    loadPlayerConfig:async()=>({cardList:{}}),
  };
  const calculation=createCalculationActions({state,elements,readPlayer:()=>player,normalizedPlayer:p=>p,
    persistResultCache:async value=>{if(persistFails)throw Error('history write failed');persisted=structuredClone(value);steps.push('invalidate');},
    renderResultCache:value=>renders.push(value),
  });
  const actions=createResourceActions({state,elements,readPlayer:()=>player,writePlayer:()=>{},
    invalidateResultCache:calculation.invalidateResultCache,hasActiveCalculation:()=>busy,
    clearPersistedResultCache:async()=>{persisted=[];steps.push('delete-history');},
    cancelPendingSave:async()=>steps.push('saved'),
    refreshPlayerProfiles:async()=>steps.push('default-profile'),initializePlayerDefaults:player=>({player,changed:false}),
    ensureCore:async()=>{steps.push('load');state.core={ready:true};},
    renderReferenceOptions:()=>{},renderConfigForms:()=>{},renderResultSummary:value=>renders.push(value),renderMetrics:()=>{},
    renderResultCache:value=>renders.push(value),setStatus:()=>{},setError:error=>errors.push(error),
  });
  return {actions,state,steps,errors,renders,confirmations,persisted:()=>persisted};
}

for(const [action,operation] of [['handleClearGameCache','game'],['handleRefreshCoreGameData','core'],['handleSyncAllGameData','all']]) {
  test(`${action} retains every profile's history and persists invalidation before touching game data`,async t=>{
    const h=setup(t),before=structuredClone(h.state.resultCache);
    await h.actions[action]();
    assert.deepEqual(h.errors,[]);
    assert.deepEqual(h.steps,['invalidate',operation,'load']);
    assert.equal(h.state.resultCache.length,2);
    assert.deepEqual(h.state.resultCache.map(({reusable,...entry})=>entry),before);
    assert.ok(h.persisted().every(entry=>entry.reusable===false));
    assert.deepEqual(h.state.lastDiagnostic,{saved:true});
  });
}

test('a failed invalidation write aborts the resource operation without deleting history',async t=>{
  const h=setup(t,{persistFails:true});
  await h.actions.handleClearGameCache();
  assert.deepEqual(h.steps,[]);
  assert.match(h.errors[0].message,/history write failed/);
  assert.deepEqual(h.persisted(),entries());
});

test('clearing player archives also deletes all historical results and in-memory snapshots',async t=>{
  const h=setup(t);
  await h.actions.handleClearLocalCache();
  assert.deepEqual(h.errors,[]);
  assert.deepEqual(h.steps,['saved','profiles','delete-history','default-profile']);
  assert.deepEqual(h.persisted(),[]);
  assert.deepEqual(h.state.resultCache,[]);
  assert.deepEqual(h.state.profileDiagnostics,{});
  assert.equal(h.state.lastDiagnostic,null);
  assert.match(h.confirmations[0],/全部玩家档案和计算历史/);
});

for (const options of [{confirm:false},{busy:true},{clearProfilesFails:true}]) {
  test(`player cleanup retains history when cancelled, busy or profile deletion fails: ${JSON.stringify(options)}`,async t=>{
    const h=setup(t,options);
    await h.actions.handleClearLocalCache();
    assert.deepEqual(h.persisted(),entries());
    assert.deepEqual(h.state.resultCache,entries());
    assert.ok(!h.steps.includes('delete-history'));
  });
}

test('0.4.3 schema-8 history loads intact, and invalidation survives a storage round trip',async t=>{
  const previous=globalThis.indexedDB;
  t.after(()=>{if(previous===undefined)delete globalThis.indexedDB;else globalThis.indexedDB=previous;});
  let value=entries();
  const request=result=>{const request={result};queueMicrotask(()=>request.onsuccess());return request;};
  const store={get:()=>request(structuredClone(value)),put:next=>{value=structuredClone(next);return request();}};
  globalThis.indexedDB={open:()=>request({transaction:()=>({objectStore:()=>store})})};
  const storage=createResultCacheStorage();
  const loaded=await storage.loadResultCache();
  assert.equal(loaded.length,2);
  assert.ok(loaded.every(entry=>entry.reusable));
  assert.deepEqual(loaded.find(entry=>entry.profileId==='A').result,entries()[0].result);
  await storage.saveResultCache(loaded.map(entry=>({...entry,reusable:false})));
  const reloaded=await storage.loadResultCache();
  assert.equal(reloaded.length,2);
  assert.ok(reloaded.every(entry=>entry.reusable===false));
  assert.deepEqual(reloaded.map(entry=>entry.result),loaded.map(entry=>entry.result));
});
