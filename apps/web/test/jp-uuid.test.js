import test from 'node:test';
import assert from 'node:assert/strict';
import {importJpUuid} from '../src/data/jp-uuid.js';
import {mergeAccountImport} from '../src/data/cn-account.js';
import {importSourceMarkup} from '../src/ui/approved/archive-flows.js';
import {flowHelpers} from '../src/ui/archive-flows.js';

const request={playerId:42,uuid:'00000000-0000-4000-8000-000000000001'};
const result={gameUid:42,name:'Test',rank:1,player:{playerId:42,cardList:{3:{level:40}},areaItem:{1:{level:2}},characterBouns:{1:{}}}};

test('JP UUID transport sends one private POST',async()=>{
  const calls=[];
  const data=await importJpUuid({apiBaseUrl:'https://calc.example',request,fetchImpl:async(url,options)=>{calls.push({url,options});return Response.json({status:'ok',data:result});}});
  assert.deepEqual(data,result);
  assert.equal(calls.length,1);
  assert.equal(calls[0].url,'https://calc.example/api/import/jp-uuid');
  assert.equal(calls[0].options.method,'POST');
  assert.equal(calls[0].options.cache,'no-store');
  assert.equal(calls[0].options.credentials,'omit');
  assert.deepEqual(JSON.parse(calls[0].options.body),request);
});

test('JP UUID import requires HTTPS and preserves local records',async()=>{
  await assert.rejects(importJpUuid({apiBaseUrl:'http://calc.example',request,fetchImpl:async()=>{throw Error('sent');}}),/HTTPS/);
  const before={server:'cn',cardList:{4:{level:30}},areaItem:{},characterBouns:{},currentEvent:7};
  const merged=mergeAccountImport(before,result,'jp');
  assert.equal(merged.server,'jp');
  assert.equal(merged.playerId,request.playerId);
  assert.deepEqual(merged.cardList,{3:{level:40},4:{level:30}});
  assert.equal(merged.currentEvent,7);
  assert.throws(()=>mergeAccountImport(before,{...result,player:{...result.player,playerId:1}},'jp'));
});

test('JP UUID choice keeps UUID out of generated markup',()=>{
  const p={server:'jp',name:'Archive'};
  const d={source:'account',server:'jp',method:'uuid',playerId:'42',uuid:request.uuid};
  const html=importSourceMarkup({...flowHelpers,d,p});
  assert.match(html,/name="import-method"[^>]*value="uuid"[^>]*checked/);
  assert.match(html,/id="import-uuid" name="password" type="password" autocomplete="current-password"/);
  assert.match(html,/id="import-id" name="username" autocomplete="username"/);
  assert.match(html,/id="toggle-import-uuid"[^>]*aria-label="显示 UUID"[^>]*><span data-icon="eye"/);
  assert.doesNotMatch(html,new RegExp(request.uuid));
  assert.match(importSourceMarkup({...flowHelpers,d:{...d,method:'public'},p}),/主乐队的公开资料/);
});
