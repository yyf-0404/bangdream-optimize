import test from 'node:test';
import assert from 'node:assert/strict';
import {createDesktopRuntime} from '../src/runtime/desktop.js';
import {createDownloadActions} from '../src/actions/download.js';
import {createResourceActions} from '../src/actions/resource.js';

test('desktop capability controls keep local resource tools and hide browser-only download and archive clearing', () => {
  const elements = Object.fromEntries(['openDesktopDownloads','clearLocalCache','syncAllGameData','refreshCoreGameData'].map(key=>[key,{hidden:false}]));
  const state = {runtime:{kind:'desktop',syncAllGameData(){},refreshCoreGameData(){}}};
  createDownloadActions({state,elements}).configureDownloadControls();
  createResourceActions({state,elements}).configureRuntimeControls();
  assert.equal(elements.openDesktopDownloads.hidden,true);
  assert.equal(elements.clearLocalCache.hidden,true);
  assert.equal(elements.syncAllGameData.hidden,false);
  assert.equal(elements.refreshCoreGameData.hidden,false);
});

test('native file saving preserves cancel, successful save and write failure, including profile exports', async () => {
  const previous = globalThis.__TAURI__, warn = console.warn;
  const calls=[]; let outcome=false;
  globalThis.__TAURI__={core:{invoke:async(command,args)=>{calls.push({command,args});if(outcome instanceof Error)throw outcome;return outcome;}}};
  console.warn=()=>{};
  try {
    const runtime=await createDesktopRuntime();
    const payload={fileName:'档案-base64.json',text:'{"v":1,"d":"example"}'};
    assert.equal(await runtime.saveJsonFile(payload),'cancelled');
    outcome=true;
    assert.equal(await runtime.saveJsonFile(payload),'saved');
    outcome=new Error('write failed');
    await assert.rejects(runtime.saveJsonFile(payload),/write failed/);
    assert.deepEqual(calls,Array(3).fill({command:'save_json_file',args:payload}));
  } finally {console.warn=warn;if(previous===undefined)delete globalThis.__TAURI__;else globalThis.__TAURI__=previous;}
});

test('download failure exposes retry and a subsequent empty response is fetched again', async () => {
  const previous=globalThis.fetch; let attempts=0;
  globalThis.fetch=async()=>{attempts++;return attempts===1?new Response('',{status:503}):new Response('[]');};
  const elements={openDesktopDownloads:{},desktopDownloadsDialog:{showModal(){}},desktopDownloadsStatus:{},desktopDownloadsList:{textContent:''},retryDesktopDownloads:{}};
  const errors=[];
  try{
    const actions=createDownloadActions({state:{runtime:{kind:'browser'}},elements,setError:e=>errors.push(e)});
    await actions.handleOpenDesktopDownloads();
    assert.equal(elements.retryDesktopDownloads.hidden,false);assert.equal(errors.length,1);
    await actions.handleOpenDesktopDownloads();
    assert.equal(elements.desktopDownloadsStatus.textContent,'没有可下载的桌面端文件');
    await actions.handleOpenDesktopDownloads();
    assert.equal(attempts,3);
  } finally {globalThis.fetch=previous;}
});

test('desktop cannot accidentally open the web download dialog or fetch a download index', async () => {
  const elements={openDesktopDownloads:{hidden:false},desktopDownloadsDialog:{showModal(){assert.fail('desktop download dialog opened');}}};
  const actions=createDownloadActions({state:{runtime:{kind:'desktop'}},elements});
  await actions.handleOpenDesktopDownloads();
  assert.equal(elements.openDesktopDownloads.hidden,true);
});


test('native result image copying sends PNG bytes through the clipboard command', async () => {
  const previous=globalThis.__TAURI__,warn=console.warn,calls=[];
  globalThis.__TAURI__={core:{invoke:async(command,args)=>calls.push({command,args})}};console.warn=()=>{};
  try {
    const runtime=await createDesktopRuntime();
    await runtime.copyImage(new Blob([new Uint8Array([137,80,78,71])],{type:'image/png'}));
    assert.deepEqual(calls,[{command:'copy_result_image',args:{bytes:[137,80,78,71]}}]);
  } finally {console.warn=warn;if(previous===undefined)delete globalThis.__TAURI__;else globalThis.__TAURI__=previous;}
});
