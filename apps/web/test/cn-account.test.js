import test from 'node:test';
import assert from 'node:assert/strict';
import {importCnAccount, mergeCnAccountImport} from '../src/data/cn-account.js';
import {importSourceMarkup} from '../src/ui/approved/archive-flows.js';
import {flowHelpers} from '../src/ui/archive-flows.js';
const result={gameUid:42,name:'Test',rank:268,channel:'android',player:{playerId:42,cardList:{3:{skillLevel:5}},areaItem:{1:{level:6}},characterBouns:{1:{}}}};
const credentials={account:'test@example.invalid',password:'test-secret',channel:'android'};
test('credential transport uses a single no-store JSON POST and never a query string',async()=>{
 const calls=[];const data=await importCnAccount({apiBaseUrl:'https://calc.example',credentials,fetchImpl:async(url,options)=>{calls.push({url,options});return Response.json({status:'ok',data:result});}});
 assert.deepEqual(data,result);assert.equal(calls.length,1);assert.equal(calls[0].url,'https://calc.example/api/import/cn-account');
 const o=calls[0].options;assert.equal(o.method,'POST');assert.equal(o.cache,'no-store');assert.equal(o.credentials,'omit');assert.equal(o.redirect,'error');assert.deepEqual(JSON.parse(o.body),credentials);
});
test('insecure remote endpoints and cancelled requests never receive credentials',async()=>{
 let calls=0;const fetchImpl=async()=>{calls++;};
 await assert.rejects(importCnAccount({apiBaseUrl:'http://calc.example',credentials,fetchImpl}),/HTTPS/);
 const c=new AbortController();c.abort();await assert.rejects(importCnAccount({credentials,signal:c.signal,fetchImpl}));assert.equal(calls,0);
});
test('missing backend and SDK failures stop without retry or relaying HTML',async()=>{
 let calls=0;await assert.rejects(importCnAccount({credentials,fetchImpl:async()=>{calls++;return new Response('<html>oops</html>');}}),/接口尚未就绪/);assert.equal(calls,1);
 await assert.rejects(importCnAccount({credentials,fetchImpl:async()=>Response.json({status:'error',message:'请核对账号密码'},{status:502})}),/核对账号密码/);
});
test('full account import replaces official collections and preserves custom cards and activity settings',()=>{
 const before={playerId:1,server:'jp',currentEvent:999,cardList:{9:{}},areaItem:{2:{}},characterBouns:{2:{}},customCards:{1000001:{uid:'custom'}},nextCustomCardId:1000002,eventPresets:{a:1},eventSongs:{a:[]}};
 const merged=mergeCnAccountImport(before,result);assert.equal(merged.server,'cn');assert.equal(merged.playerId,42);assert.deepEqual(merged.cardList,result.player.cardList);assert.deepEqual(merged.areaItem,result.player.areaItem);assert.equal(merged.currentEvent,999);assert.deepEqual(merged.customCards,before.customCards);assert.deepEqual(merged.eventPresets,before.eventPresets);assert.equal(merged.nextCustomCardId,1000002);assert.deepEqual(before.cardList,{9:{}});assert.ok(!JSON.stringify(merged).includes('test-secret'));
});
test('incomplete or mismatched account data cannot clear an archive',()=>{
 for(const data of [{...result,gameUid:43},{...result,player:{...result.player,areaItem:null}},{...result,channel:'unknown'}])assert.throws(()=>mergeCnAccountImport({},data));
});
test('CN credentials form keeps password out of markup and requires explicit channel; public import remains available',()=>{
 const p={server:'cn',name:'Archive'};const d={source:'account',server:'cn',method:'credentials',account:'test',password:'test-secret',channel:'',playerId:''};
 const html=importSourceMarkup({...flowHelpers,d,p});assert.match(html,/type="password"/);assert.doesNotMatch(html,/test-secret/);assert.match(html,/name="import-channel"/);assert.doesNotMatch(html,/name="import-channel"[^>]*checked/);assert.match(html,/主乐队公开资料/);assert.match(html,/>bili安卓</);assert.doesNotMatch(html,/>Android</);
 const publicHtml=importSourceMarkup({...flowHelpers,d:{...d,method:'public'},p});assert.match(publicHtml,/id="import-id"/);assert.doesNotMatch(publicHtml,/type="password"/);
 const jp=importSourceMarkup({...flowHelpers,d:{...d,server:'jp'},p});assert.doesNotMatch(jp,/type="password"/);
});

test('desktop uses its native account command and discards a cancelled native result', async()=>{
 const {createDesktopRuntime}=await import('../src/runtime/desktop.js');
 const previous=globalThis.__TAURI__,warn=console.warn;let calls=0,controller;
 globalThis.__TAURI__={core:{invoke:async(command,args)=>{assert.equal(command,'import_cn_account');assert.deepEqual(args,{credentials});calls++;controller?.abort();return result;}}};console.warn=()=>{};
 try{
  const runtime=await createDesktopRuntime();assert.deepEqual(await runtime.importCnAccount(credentials),result);
  controller=new AbortController();await assert.rejects(runtime.importCnAccount(credentials,{signal:controller.signal}));
  await assert.rejects(runtime.importCnAccount(credentials,{signal:controller.signal}));assert.equal(calls,2);
 }finally{console.warn=warn;if(previous===undefined)delete globalThis.__TAURI__;else globalThis.__TAURI__=previous;}
});
