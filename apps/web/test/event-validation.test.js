import test from 'node:test';
import assert from 'node:assert/strict';
import {createEventContext} from '../src/app/event.js';
import {isSupportedEventType, isHiddenEventId} from '../src/models/event.js';
import {validateAt, validationError} from '../src/models/validation-error.js';
import {copyImageToClipboard} from '../src/ui/clipboard.js';

test('highest-score candidates and selected event validation share the same restrictions', () => {
  let mode='maximize';
  const types=['challenge','versus','medley','mission_live','live_try','festival'];
  const events=Object.fromEntries(types.map((eventType,i)=>[i+1,{eventType}]));
  const ctx=createEventContext({state:{core:{events}},elements:{calculationMode:{querySelector:()=>({value:mode})}},
    normalizedCalculationMode:v=>v,isSupportedEventType,isHiddenEventId});
  assert.deepEqual(Object.values(ctx.supportedEventRecords()).map(e=>e.eventType), types.slice(0,3));
  assert.throws(()=>ctx.assertSupportedEvent({eventType:'festival'}), e=>e.validationTarget.selector==='#activity-event-select');
  mode='ptMaximize';
  assert.equal(Object.keys(ctx.supportedEventRecords()).length,6);
  assert.doesNotThrow(()=>ctx.assertSupportedEvent({eventType:'festival'}));
});

test('validation preserves a precise field destination inside a section check', () => {
  assert.throws(()=>validateAt('#section',()=>{throw validationError('目标无效','#target');}),
    e=>e.validationTarget.selector==='#target'&&e.validationTarget.page==='activity');
  assert.throws(()=>validateAt('#items',()=>{throw new Error('未选择道具');}),
    e=>e.message==='未选择道具'&&e.validationTarget.selector==='#items');
});

test('image clipboard starts in the click activation, before PNG rendering resolves', async () => {
  const previous=Object.getOwnPropertyDescriptor(globalThis,'navigator'),oldItem=globalThis.ClipboardItem;
  let captured,resolve;
  const png=new Promise(r=>resolve=r);
  try {
    globalThis.ClipboardItem=class {constructor(data){this.data=data;}};
    Object.defineProperty(globalThis,'navigator',{configurable:true,value:{clipboard:{write(items){captured=items;return Promise.resolve();}}}});
    const copied=copyImageToClipboard(png);
    assert.strictEqual(captured[0].data['image/png'],png);
    resolve(new Blob(['png'],{type:'image/png'}));await copied;
    let native;
    await copyImageToClipboard(png,{copyImage:async b=>{native=b;}});
    assert.equal(native.type,'image/png');
  } finally {
    if(previous)Object.defineProperty(globalThis,'navigator',previous);else delete globalThis.navigator;
    globalThis.ClipboardItem=oldItem;
  }
});
