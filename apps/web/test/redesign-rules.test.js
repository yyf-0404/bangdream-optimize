import test from 'node:test';
import assert from 'node:assert/strict';
import {gameText,languageOrder,saveProfilePreference,profilePreference} from '../src/shared/preferences.js';
import {defaultFilters,filterCards,cardMatchesReleaseFilter,cardTypeFilterValue,cardTypeLabel,resolveCardCover,comparator,normalizeCardSort,groupCards} from '../src/components/cards/rules.js';
import {cardSkillFilterValue, normalizeSkillBonusFilter} from '../src/components/cards/skill.js';
import {toggleSelectionScope} from '../src/components/cards/catalog.js';
import {planBulk} from '../src/components/cards/bulk.js';
import {cardModel,cardConfig,catalogModels,allCardModels} from '../src/components/cards/model.js';
import {bonusApplication} from '../src/pages/activity/activity.js';
import {importChanges} from '../src/pages/archive/import-review.js';

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
test('skill filtering groups maximum score bonuses, with PERFECT rate-up in the 100–125% group', () => {
  const base = { characterId: 1, rarity: 4, attribute: 'pure', owned: true, skill: 1, releaseDates: { cn: now - week } };
  const card = (id, skillId, activateEffectTypes, unificationActivateEffectValue) => ({
    ...base, id, skillId, skillRecord: { activationEffect: { activateEffectTypes, unificationActivateEffectValue } },
  });
  const cards = [
    card(1, 28, { score: { activateEffectValue: [100, 110] } }, 125),
    card(2, 61, { score: { activateEffectValue: [100] }, score_rate_up_with_perfect: { activateEffectValue: [0] } }),
    card(3, 8, { score: { activateEffectValue: [80, 100, 110] } }),
    card(4, 1, { judge: { activateEffectValue: [120] } }),
    { ...base, id: 5 },
    card(6, 75, { score: { activateEffectValue: [150] } }),
    card(7, 76, { score: { activateEffectValue: [145] } }),
    card(8, 77, { score: { activateEffectValue: [100] } }),
  ];
  assert.deepEqual(cards.map(cardSkillFilterValue), ['100-125', '100-125', '100-125', 'under-100', undefined,
    '150-plus', '130-145', '100-125']);
  const filters = defaultFilters([{ id: 1 }], 'cn');
  filters.server = null;
  assert.equal(filters.skillBonus.size, 4);
  assert.deepEqual(filterCards(cards, filters).map(card => card.id), [1, 2, 3, 4, 5, 6, 7, 8]);
  filters.skillBonus = new Set(['150-plus', '130-145']);
  assert.deepEqual(filterCards(cards, filters).map(card => card.id), [6, 7]);
  filters.skillBonus = new Set(['100-125']);
  assert.deepEqual(filterCards(cards, filters).map(card => card.id), [1, 2, 3, 8]);
  filters.skillBonus.clear();
  assert.deepEqual(filterCards(cards, filters), []);
});
test('saved exact skill bonuses and rate-up restore to their grouped filters', () => {
  assert.deepEqual(normalizeSkillBonusFilter(new Set(['150', '135', '125', 'rateup', '80', '130-145'])),
    new Set(['150-plus', '130-145', '100-125', 'under-100']));
  assert.deepEqual(normalizeSkillBonusFilter(null), new Set());
});
test('card type filtering combines festival types and groups all remaining types as other', () => {
  const types = ['dreamfes', 'kirafes', 'limited', 'permanent', 'birthday', 'event', 'initial', 'campaign',
    'others', 'special', undefined, 'future'];
  const cards = types.map((type, index) => ({ id: index + 1, characterId: 1, rarity: 4, attribute: 'pure',
    owned: true, record: { type }, skillRecord: { activationEffect: { activateEffectTypes: {
      score: { activateEffectValue: [index === 1 ? 150 : 120] },
    } } } }));
  assert.deepEqual(cards.map(cardTypeFilterValue), ['dreamfes', 'dreamfes', 'limited', 'permanent', 'birthday',
    'event', ...Array(6).fill('other')]);
  assert.deepEqual(cards.map(cardTypeLabel), ['梦限', '闪限', '普限', '常驻', '生日', '活动奖励',
    '初始卡', '宣传／纪念发放', '其他', '特殊卡', '其他', '其他']);
  const filters = defaultFilters([{ id: 1 }], 'cn');
  filters.server = null;
  assert.equal(filters.cardType.size, 6);
  filters.cardType = new Set(['dreamfes', 'other']);
  assert.deepEqual(filterCards(cards, filters).map(card => card.id), [1, 2, 7, 8, 9, 10, 11, 12]);
  filters.skillBonus = new Set(['150-plus']);
  assert.deepEqual(filterCards(cards, filters).map(card => card.id), [2]);
  filters.cardType.clear();
  assert.deepEqual(filterCards(cards, filters), []);
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
test('activity bonus picker includes custom cards without a server release date',()=>{
  const customId=1_000_000_001;
  const zero={performance:0,technique:0,visual:0};
  const core={cards:{1:{characterId:2,rarity:5,attribute:'pure',releasedAt:[null,null,null,Date.now()-1000,null]}},characters:{2:{bandId:1,characterName:['角色']}}};
  const player={server:'cn',cardList:{1:{level:60}},customCards:{[customId]:{
    enabled:true,definition:{cardId:customId,characterId:2,bandId:1,rarity:5,attribute:'pure',levelStats:{60:zero},trainingStat:zero,episodeStats:[zero,zero]},
    growth:{level:60,skillLevel:5,training:true,episodes:[true,true],limitBreakRank:0},editor:{name:'试算卡'},
  }}};
  assert.deepEqual(catalogModels(core,player,'profile').map(c=>c.id),[1]);
  const choices=allCardModels(core,player,'profile');
  assert.deepEqual(choices.map(c=>c.id),[1,customId]);
  assert.equal(choices[1].displayId,'C-001');
  assert.equal(resolveCardCover(choices,'cn',{mode:'manual',cardId:customId}).card.id,customId);
  assert.equal(resolveCardCover(choices,'cn').card.id,1);
  assert.equal(resolveCardCover([{...choices[0],releaseDates:{}},choices[1]],'cn').card.id,1);
  assert.equal(resolveCardCover([choices[1]],'cn').card.id,customId);
  assert.deepEqual(filterCards(choices,defaultFilters([{id:2,band:1,name:'角色'}],'cn')).map(c=>c.id),[1,customId]);
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
