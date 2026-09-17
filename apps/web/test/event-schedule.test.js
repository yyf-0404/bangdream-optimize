import test from 'node:test';
import assert from 'node:assert/strict';
import {createPlayerModel} from '../src/models/player.js';
import {defaultEventTypeForMode, supportedEventTypeOrDefault} from '../src/models/event.js';
import {eventCardPoolInfo, restrictEventCardPool} from '../src/models/event-card-pool.js';

const model = createPlayerModel({
  eventWithParameterBonusFix: event => event,
  defaultEventTypeForMode,
  supportedEventTypeOrDefault,
});
const event = {eventType:'medley', startAt:['100',null,'150','200',null], endAt:['300',null,'350','400',null]};
const cards = {1:{releasedAt:[50,null,100,150,null]},2:{releasedAt:[450,null,450,450,null]}};
const profile = preset => ({server:'cn', currentEvent:305, calculationMode:'maximize',
  eventAvailableCardsOnly:true, eventPresets:{305:preset}, cardList:{1:{skillLevel:5},2:{skillLevel:5}}});

test('a cached preset without dates cannot hide current catalog dates from display or calculation',()=>{
  for(const calculationMode of ['maximize','ptMaximize']){
    const player = {...profile({eventType:'medley', attributes:[{attribute:'cool',percent:42}]}),calculationMode};
    const before = structuredClone(player);
    const snapshot = model.editableEventSnapshot(305, player, {305:event});
    assert.equal(snapshot.startAt[3],200);
    assert.equal(snapshot.endAt[3],400);
    assert.deepEqual(snapshot.attributes,player.eventPresets[305].attributes);
    assert.equal(eventCardPoolInfo(player,snapshot,cards).endAt,400);
    assert.deepEqual(Object.keys(restrictEventCardPool(player,snapshot,cards).player.cardList),['1']);
    assert.deepEqual(player,before);
    assert.equal(event.endAt[3],'400');
  }
});

test('stale per-server dates are refreshed; missing catalog entries only fall back to the same server',()=>{
  const player = profile({...event,endAt:['250','500',null,'270',null]});
  const snapshot = model.editableEventSnapshot(305,player,{305:event});
  assert.deepEqual(snapshot.endAt,[300,500,350,400,null]);
  for(const [server,endAt]of [['jp',300],['en',500],['tw',350],['cn',400],['kr',null]]){
    assert.equal(eventCardPoolInfo({...player,server},snapshot,cards).endAt,endAt);
  }
});

test('an offline preset retains its schedule and custom events never inherit the official cutoff',()=>{
  const player = profile(event);
  assert.equal(eventCardPoolInfo(player,model.editableEventSnapshot(305,player),cards).endAt,400);
  const custom = {...player,currentEvent:0,eventPresets:{0:event},eventOverrides:{0:{eventType:'medley'}}};
  assert.equal(eventCardPoolInfo(custom,model.editableEventSnapshot(0,custom,{0:event}),cards).endAt,null);
  const undated = {...event,endAt:[300,null,null,null,null]};
  const missing = profile(undated);
  assert.equal(eventCardPoolInfo(missing,model.editableEventSnapshot(305,missing,{305:undated}),cards).endAt,null);
});

test('a scalar preset date is only used for its selected server when reconciling a server array',()=>{
  const player = profile({...event,endAt:'400'});
  const snapshot = model.editableEventSnapshot(305,player,{305:{...event,endAt:['300',null,null,null,null]}});
  assert.deepEqual(snapshot.endAt,[300,null,null,400,null]);
});
