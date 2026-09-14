import assert from 'node:assert/strict';
import test from 'node:test';
import {searchOptions,moveOption,placeSelectPopup} from '../src/ui/select-model.js';

test('activity search prioritizes exact IDs and combines name tokens without changing release order',()=>{
 const rows=[{value:'341',label:'#341 · 夏日 Live'},{value:'34',label:'#34 · 夏日 Live'},{value:'134',label:'#134 · 秋日 Live'},{value:'9',label:'#9 · 夏日 Live',hidden:true}];
 assert.deepEqual(searchOptions(rows,'３４').map(o=>o.value),['34','341','134']);
 assert.deepEqual(searchOptions(rows,'夏日 live').map(o=>o.value),['341','34']);
 assert.deepEqual(searchOptions(rows,'#34').map(o=>o.value),['34','341']);
 assert.equal(searchOptions(rows,'不存在').length,0);
});
test('keyboard navigation skips disabled options, stays within bounds, and handles empty results',()=>{
 const rows=[{disabled:true},{value:'a'},{disabled:true},{value:'b'}];
 assert.equal(moveOption(rows,-1,1),1);assert.equal(moveOption(rows,1,1),3);
 assert.equal(moveOption(rows,3,1),3);assert.equal(moveOption(rows,3,-1),1);
 assert.equal(moveOption(rows,3,-Infinity),1);assert.equal(moveOption(rows,1,Infinity),3);
 assert.equal(moveOption([],0,1),-1);assert.equal(moveOption([{disabled:true}],0,1),-1);
});
test('popup opens above a bottom-edge control and stays inside narrow, offset visual viewports',()=>{
 const anchor={left:292,width:160,top:690,bottom:734};
 const result=placeSelectPopup(anchor,{width:375,height:812},{preferredWidth:360,desiredHeight:360});
 assert.equal(result.upward,true);assert.equal(result.width,359);assert.equal(result.left,8);
 assert.ok(result.top>=8);assert.ok(result.top+result.maxHeight<anchor.top);
 const keyboard=placeSelectPopup({left:30,width:300,top:380,bottom:424},{left:0,top:200,width:375,height:300},{preferredWidth:360,desiredHeight:360});
 assert.ok(keyboard.top>=208);assert.ok(keyboard.top+keyboard.maxHeight<=492);
 const coveredAnchor=placeSelectPopup({left:30,width:300,top:690,bottom:734},{top:200,width:375,height:300});
 assert.ok(coveredAnchor.top>=208);assert.ok(coveredAnchor.top+coveredAnchor.maxHeight<=492);
});
test('short lists use their natural height below the trigger rather than a large empty panel',()=>{
 const result=placeSelectPopup({left:40,width:150,top:100,bottom:136},{width:1280,height:900},{desiredHeight:120});
 assert.equal(result.upward,false);assert.equal(result.top,142);assert.equal(result.maxHeight,120);
});
