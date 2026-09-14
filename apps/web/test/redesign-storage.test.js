import test from 'node:test';
import assert from 'node:assert/strict';
import { createPlayerStore } from '../src/app/player.js';
import { cardModel, cardConfig } from '../src/ui/cards/model.js';
import { cardSkillInfo } from '../src/ui/cards/skill.js';

const deferred=()=>{let resolve;const promise=new Promise(r=>resolve=r);return {promise,resolve};};
function setup(savePlayerConfig){
  const state={activePlayerProfileId:'A',playerSaveSequence:0,playerSaveQueue:Promise.resolve(),runtime:{savePlayerConfig},core:{events:{}}};
  const store=createPlayerStore({state,playerJson:{value:'{}'},normalizePlayer:p=>structuredClone(p),cacheEventPresetFromCore:()=>false,activityModeForEvent:()=> 'single',ensureSongListForMode(){},recentUnfinishedEvent:()=>null,renderPlayerProfileControls(){},onError(){}});
  return {state,store};
}
test('queued saves retain profile identity and serialize writes across a switch',async()=>{
  const gate=deferred(),writes=[];
  const {state,store}=setup(async(p,id)=>{writes.push([id,p.playerId]);if(p.playerId===1)await gate.promise;});
  const a=store.savePlayerNow({playerId:1}),a2=store.savePlayerNow({playerId:2});
  state.activePlayerProfileId='B';const b=store.savePlayerNow({playerId:3});
  gate.resolve();await Promise.all([a,a2,b]);assert.deepEqual(writes,[['A',1],['A',2],['B',3]]);
});
test('save failure surfaces to the caller and a later save can recover',async()=>{
  let count=0;const {store}=setup(async()=>{if(++count===1)throw new Error('quota');});
  await assert.rejects(store.savePlayerNow({playerId:1}),/quota/);
  await store.savePlayerNow({playerId:2});assert.equal(count,2);
});
test('unknown card metadata preserves every raw growth field',()=>{
  const config={level:60,skillLevel:4,training:false,illustTrainingStatus:true,episodes:[false,true],limitBreakRank:3};
  const c=cardModel({}, {cardList:{999999:config}},999999);
  assert.equal(c.unknown,true);assert.deepEqual(cardConfig(c),config);assert.equal(c.growth.illustTrained,true);
});
test('skill brief and detail use the selected skill level',()=>{
  const skillRecord={description:['{0} seconds'],duration:[4,4.5,5,5.5,6],activationEffect:{activateEffectTypes:{score:{activateEffectValue:[80,85,90,95,100]}}}};
  const low=cardSkillInfo({skill:2,skillRecord}),high=cardSkillInfo({skill:5,skillRecord});
  assert.equal(low.score,85);assert.equal(low.description,'4.5 seconds');assert.equal(high.score,100);
});
