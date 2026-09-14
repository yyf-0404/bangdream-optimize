import test from 'node:test';
import assert from 'node:assert/strict';
import {gameText,languageOrder,saveProfilePreference,profilePreference} from '../src/ui/preferences.js';
import {defaultFilters,filterCards,cardMatchesReleaseFilter,resolveCardCover,comparator,normalizeCardSort,groupCards} from '../src/ui/cards/rules.js';
import {toggleSelectionScope} from '../src/ui/cards/catalog.js';
import {planBulk} from '../src/ui/cards/bulk.js';
import {cardModel,cardConfig} from '../src/ui/cards/model.js';
import {bonusApplication} from '../src/ui/activity.js';
import {importChanges} from '../src/ui/import-review.js';

test('game text prefers the application language then Chinese-first fallback',()=>{
  const texts=['日本語','English','繁體','简体','한국어'];
  assert.equal(gameText(texts,'','en'),'English');
  assert.equal(gameText([null,null,'繁體',null,null],'','en'),'繁體');
  assert.equal(gameText(['日文','英語',null,'简体',null],'','zh-TW'),'简体');
  assert.equal(gameText([null,'',null,' ',null],'卡牌 42'),'卡牌 42');
  assert.deepEqual(languageOrder('ja'),['ja','zh-CN','zh-TW','en','ko']);
});
test('profile preferences remain isolated from other profiles and from exported player fields',()=>{
  saveProfilePreference('a','cover',{cardId:1});saveProfilePreference('b','cover',{cardId:2});
  assert.equal(profilePreference('a','cover',{}).cardId,1);assert.equal(profilePreference('b','cover',{}).cardId,2);
});
const now=1800000000000,week=7*24*3600*1000;
test('card release filter is inclusive at 168 hours and never borrows another server date',()=>{
  assert.equal(cardMatchesReleaseFilter({releaseDates:{cn:now+week}},'cn',now),true);
  assert.equal(cardMatchesReleaseFilter({releaseDates:{cn:now+week+1}},'cn',now),false);
  assert.equal(cardMatchesReleaseFilter({releaseDates:{jp:now-1}},'cn',now),false);
});
test('held cover ignores publication/server, skips 2 star only in auto mode',()=>{
  const cards=[{id:1,rarity:5,owned:true,releaseDates:{jp:now}},{id:2,rarity:2,owned:true,releaseDates:{jp:now+week}},{id:3,rarity:5,owned:false,releaseDates:{jp:now+week*2}}];
  assert.equal(resolveCardCover(cards,'kr').card.id,1);
  assert.equal(resolveCardCover(cards,'kr',{mode:'manual',cardId:2}).card.id,2);
});
test('release sort keeps missing dates last in both directions and uses ID tie-break',()=>{
  const a={id:1,releaseDates:{jp:10}},b={id:2,releaseDates:{cn:10}},missing={id:9};
  for(const order of ['release-desc','release-asc'])assert.deepEqual([missing,a,b].sort(comparator(order)).map(c=>c.id),[2,1,9]);
});
test('card sort preferences separate rules from direction and migrate existing selections',()=>{
  for(const rule of ['release','rarity','id'])for(const direction of ['asc','desc']) {
    assert.deepEqual(normalizeCardSort(`${rule}-${direction}`),{sort:rule,sortDirection:direction});
    assert.deepEqual(normalizeCardSort(rule,direction),{sort:rule,sortDirection:direction});
  }
  assert.deepEqual(normalizeCardSort(),{sort:'owned-release',sortDirection:'desc'});
  assert.deepEqual(normalizeCardSort('release-asc','desc'),{sort:'release',sortDirection:'desc'});
});
test('card sort direction reverses numeric and ownership keys without changing group order',()=>{
  const cards=[
    {id:1,owned:true,rarity:5,releaseDates:{jp:20}},
    {id:2,owned:false,rarity:4,releaseDates:{cn:30}},
    {id:3,owned:true,rarity:5,releaseDates:{jp:10}},
    {id:4,owned:true,rarity:5},
  ];
  const ids=(sort,direction)=>[...cards].sort(comparator(sort,direction)).map(c=>c.id);
  assert.deepEqual(ids('owned-release','desc'),[1,3,4,2]);
  assert.deepEqual(ids('owned-release','asc'),[2,3,1,4]);
  assert.deepEqual(ids('release','desc'),[2,1,3,4]);
  assert.deepEqual(ids('release','asc'),[3,1,2,4]);
  assert.deepEqual(ids('rarity','desc'),[1,3,4,2]);
  assert.deepEqual(ids('rarity','asc'),[2,3,1,4]);
  assert.deepEqual(ids('id','asc'),[1,2,3,4]);
  assert.deepEqual(ids('id','desc'),[4,3,2,1]);
  const grouped=groupCards(cards,'rarity','release',[],'asc');
  assert.deepEqual(grouped.map(g=>g.key),['5','4']);
  assert.deepEqual(grouped[0].cards.map(c=>c.id),[3,1,4]);
  assert.deepEqual(cards.map(c=>c.id),[1,2,3,4]);
});
test('bulk all toggles complete filtered scope including unmounted cards, preserving hidden selection',()=>{
  const scope=Array.from({length:2100},(_,i)=>i+1),before=new Set([9000,1]);
  const all=toggleSelectionScope(before,scope);assert.equal(all.size,2101);
  assert.deepEqual([...toggleSelectionScope(all,scope)],[9000]);assert.deepEqual([...before],[9000,1]);
});
test('growth edits leave unchecked fields and unowned cards unchanged; add never overwrites held cards',()=>{
  const c={id:1,owned:true,level:60,skill:5,maxLevel:60,capabilities:[[false,true],[1,1]],growth:{trained:true,illustTrained:false,mastery:3,episodes:[true,false]}};
  const missing={...c,id:2,owned:false};
  const [change]=planBulk([c,missing],'edit',{skill:2},['skill']);
  assert.equal(change.next.skill,2);assert.deepEqual(change.next.growth,c.growth);assert.equal(c.skill,5);
  const [added]=planBulk([c,missing],'add',{},[]);assert.equal(added.card.id,2);assert.equal(added.next.growth.mastery,3);
});
test('formal card model separates training growth from illustration and reads real skills',()=>{
  const core={cards:{1:{characterId:2,rarity:5,levelLimit:60,skillId:7,stat:{training:{performance:1},episodes:[{performance:1},{performance:0,technique:0,visual:0}]}}},characters:{2:{characterName:['名前'],bandId:1}},skills:{7:{description:['技能']}}};
  const config={level:55,skillLevel:2,training:true,illustTrainingStatus:false,limitBreakRank:3,episodes:[false,true]};
  const c=cardModel(core,{cardList:{1:config}},1);assert.equal(c.growth.illustTrained,false);assert.equal(c.growth.trained,true);assert.equal(c.skillRecord,core.skills[7]);assert.deepEqual(cardConfig(c),config);
});
test('card detail includes the trained maximum instead of displaying 60 / 50',()=>{
  const c=cardModel({cards:{1:{levelLimit:50,stat:{1:{},60:{},training:{levelLimit:10}},rarity:5}}},{cardList:{1:{level:60,training:true}}},1);
  assert.equal(c.maxLevel,60);assert.equal(c.level,60);
});
test('bonus application follows calculation target and live variant',()=>{
  assert.equal(bonusApplication('challenge','ptMaximize','solo'),'point');
  assert.equal(bonusApplication('challenge','ptMaximize','challenge_cp'),'stat');
  assert.equal(bonusApplication('festival','ptMaximize','solo'),'point');
  assert.equal(bonusApplication('festival','ptMaximize','festival'),'stat');
  assert.equal(bonusApplication('mission_live','maximize','solo'),'stat');
});
test('import review counts concrete changes without mutating either draft',()=>{
  const before={cardList:{1:{level:1},2:{level:5}}},after={cardList:{1:{level:2},3:{level:5}}};
  assert.deepEqual(importChanges(before,after).cardList,{added:1,changed:1,removed:1});assert.equal(before.cardList[1].level,1);
});
