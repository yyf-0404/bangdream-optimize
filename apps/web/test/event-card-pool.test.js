import test from 'node:test';
import assert from 'node:assert/strict';
import {eventCardPoolInfo, restrictEventCardPool} from '../src/models/event-card-pool.js';

const date=(cn,jp=null)=>[jp,null,null,cn,null];
const event={startAt:date(200),endAt:date(400)};
const cards=Object.fromEntries([
  [1,date(100)], [2,date(300)], [3,date(400)], [4,date(401)],
  [5,date(null,100)], [6,date('399')], [7,date('invalid')], [8,date(0)],
].map(([id,releasedAt])=>[id,{releasedAt}]));
const profile=()=>({server:'cn',currentEvent:1,calculationMode:'maximize',eventAvailableCardsOnly:true,
  cardList:Object.fromEntries(Object.keys(cards).map(id=>[id,{skillLevel:5}])),
  customCards:{1000001:{enabled:true,definition:{characterId:1}}},areaItem:{1:{level:5}}});

test('event cutoff uses only the selected server and strictly precedes its end, including mid-event releases',()=>{
  assert.deepEqual(eventCardPoolInfo(profile(),event,cards).cardIds,['1','2','6']);
  const future={...event,endAt:date(Date.now()+86400000*100)};
  assert.ok(eventCardPoolInfo(profile(),future,{...cards,4:{releasedAt:date(Date.now()+86400000)}}).cardIds.includes('4'));
});
test('server-scoped scalar end timestamps match the array representation',()=>{
  for(const endAt of [400,'400']){
    assert.deepEqual(eventCardPoolInfo(profile(),{endAt},cards).cardIds,['1','2','6']);
    assert.equal(eventCardPoolInfo(profile(),{endAt},cards).endAt,400);
  }
});
test('both automatic searches reduce only the submitted snapshot; custom cards are excluded without mutating inventory',()=>{
  for(const calculationMode of ['maximize','ptMaximize']){
    const source={...profile(),calculationMode},before=structuredClone(source);
    const pool=restrictEventCardPool(source,event,cards);
    assert.deepEqual(Object.keys(pool.player.cardList),['1','2','6']);
    assert.equal(pool.player.customCards[1000001].enabled,false);
    assert.deepEqual(source,before);
    assert.deepEqual(pool.player.areaItem,source.areaItem);
  }
});
test('disabled or non-search modes preserve the original player, even for custom or undated events',()=>{
  for(const player of [{...profile(),eventAvailableCardsOnly:false},...['ptEvaluate','scoreRange'].map(calculationMode=>({...profile(),calculationMode}))]){
    assert.equal(restrictEventCardPool(player,{},cards).player,player);
  }
});
test('missing local end and custom events use the full pool, including enabled custom cards, without other-server dates',()=>{
  for(const calculationMode of ['maximize','ptMaximize'])for(const [currentEvent,ev]of [[1,{endAt:date(null,400)}],[1,{}],[1,{endAt:date('invalid')}],[0,event]]){
    const player={...profile(),calculationMode,currentEvent},before=structuredClone(player);
    const pool=restrictEventCardPool(player,ev,cards);
    assert.equal(pool.player,player);
    assert.equal(pool.player.customCards[1000001].enabled,true);
    assert.deepEqual(player,before);
    assert.deepEqual(pool.restriction.cardIds,Object.keys(player.cardList));
    assert.equal(pool.restriction.fallback,'missing-event-end');
    assert.equal(pool.restriction.endAt,null);
  }
});
test('a known end with no eligible cards still fails at the scope control',()=>{
  assert.throws(()=>restrictEventCardPool(profile(),event,{}),e=>e.validationTarget?.selector==='#event-available-cards-only');
});
