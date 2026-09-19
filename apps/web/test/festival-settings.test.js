import test from 'node:test';
import assert from 'node:assert/strict';
import {normalizePtMaximizeConfig} from '../src/models/player-settings.js';
import {calculationMarkup} from '../src/ui/approved/calculation-template.js';
import {createResultCacheStorage, resultCacheInvalidationReason} from '../src/data/result-cache.js';

test('imported festival settings retain rank and victory while dropping obsolete score inputs',()=>{
  const ptMaximize=normalizePtMaximizeConfig({festivalTeamRank:3,festivalWon:false,festivalTeammateMode:'individual',festivalTeammateScores:[null,-1,9000000,0]});
  assert.equal(ptMaximize.festivalTeamRank,3);
  assert.equal(ptMaximize.festivalWon,false);
  assert.equal('festivalTeammateMode' in ptMaximize,false);
  assert.equal('festivalTeammateScores' in ptMaximize,false);
  const render=(t,l)=>calculationMarkup({state:{ptMaximize},server:'cn',m:'ptMaximize',t,l,R:{variants:{[t]:[l]},names:{[l]:l}}});
  const festival=render('festival','festival');
  assert.match(festival,/队内个人排名/);
  assert.match(festival,/队伍结果/);
  assert.doesNotMatch(festival,/队友|festivalTeammate/);
  assert.match(festival,/<option value="3" selected>/);
  assert.match(festival,/value="false" checked/);
  const cooperative=render('mission_live','cooperative');
  assert.match(cooperative,/四位队友共用/);
  assert.match(cooperative,/ptMaximize\.teammates\.0\.expectedStat/);
});

test('legacy 5v5 history survives storage migration but cannot be reused; corrected and solo results can',async t=>{
  const previous=globalThis.indexedDB;
  t.after(()=>{if(previous===undefined)delete globalThis.indexedDB;else globalThis.indexedDB=previous;});
  const base={cacheVersion:8,profileId:'A',calculationMode:'ptMaximize'};
  let value=[
    {...base,key:'legacy-summary',result:{liveVariant:'festival',scenario:{festival:{teammateScores:0,teamRank:0,won:true}}}},
    {...base,key:'legacy-request',diagnostic:{calculationRequest:{festival:{teammateScores:[100,200,300,400]}}}},
    {...base,key:'corrected',result:{liveVariant:'festival',scenario:{festival:{teamRank:0,won:true}}}},
    {...base,key:'solo',result:{liveVariant:'solo',eventType:'festival'}},
    {...base,key:'cooperative',result:{liveVariant:'cooperative'}},
  ];
  const request=result=>{const r={result};queueMicrotask(()=>r.onsuccess());return r;};
  const store={get:()=>request(structuredClone(value)),put:next=>{value=structuredClone(next);return request();}};
  globalThis.indexedDB={open:()=>request({transaction:()=>({objectStore:()=>store})})};
  const storage=createResultCacheStorage();
  const loaded=await storage.loadResultCache();
  assert.equal(loaded.length,5);
  for(const entry of loaded){
    assert.deepEqual(entry.result,value.find(old=>old.key===entry.key).result);
    assert.equal(entry.reusable,!entry.key.startsWith('legacy'));
    if(!entry.reusable)assert.match(resultCacheInvalidationReason(entry),/5v5 PT 计算已修正/);
  }
  await storage.saveResultCache(loaded);
  const reloaded=await storage.loadResultCache();
  assert.deepEqual(reloaded.map(e=>[e.key,e.reusable]),loaded.map(e=>[e.key,e.reusable]));
});
