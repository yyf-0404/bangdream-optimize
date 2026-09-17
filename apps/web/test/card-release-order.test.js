import test from 'node:test';
import assert from 'node:assert/strict';
import {cardReleaseOrder, comparator, groupCards, resolveCardCover, releaseOf} from '../src/ui/cards/rules.js';

const card = (id, releaseDates, extra = {}) => ({id, releaseDates, rarity:5, owned:true, ...extra});
const bridgeCards = () => [
  card(2480,{jp:Date.UTC(2026,5,30)}),
  card(2353,{jp:Date.UTC(2025,11,31),cn:Date.UTC(2026,6,16)},{owned:false,rarity:2}),
  card(10052,{cn:Date.UTC(2026,6,10)}),
];
const ids = (cards, direction='desc', order=cardReleaseOrder(cards)) =>
  [...cards].sort(comparator('release',direction,order)).map(card=>card.id);

test('a shared release bridges exclusive cards before the oldest-date fallback',()=>{
  const cards=bridgeCards(),order=cardReleaseOrder(cards);
  assert.deepEqual(ids(cards,'desc',order),[2480,2353,10052]);
  assert.deepEqual(ids(cards,'asc',order),[10052,2353,2480]);
  // The bridge is unowned and a different rarity; it must survive both filters.
  const held=cards.filter(card=>card.owned);
  for(const rule of ['release','owned-release','rarity']) {
    assert.deepEqual(groupCards(held,'rarity',rule,[],'desc',order)[0].cards.map(c=>c.id),[2480,10052]);
    assert.deepEqual(groupCards(held,'none',rule,[],'asc',order)[0].cards.map(c=>c.id),[10052,2480]);
  }
  assert.equal(resolveCardCover(cards,'cn').card.id,2480);
  assert.equal(resolveCardCover(cards,'cn',{mode:'manual',cardId:10052}).card.id,10052);
});

test('indirect order can cross every server without a directly shared date',()=>{
  const cards=[card(1,{jp:60}),card(2,{jp:50,cn:50}),card(3,{cn:40,en:40}),
    card(4,{en:30,tw:30}),card(5,{tw:20,kr:20}),card(6,{kr:10})];
  const order=cardReleaseOrder(cards);
  assert.deepEqual(ids(cards,'desc',order),[1,2,3,4,5,6]);
  assert.deepEqual(ids([cards[5],cards[0]],'desc',order),[1,6]);
});

test('conflicting lower-priority releases never reverse higher-priority relationships',()=>{
  const servers=['jp','cn','en','tw','kr'];
  for(let i=0;i<servers.length-1;i++) {
    const higher=servers[i],lower=servers[i+1];
    const cards=[card(1,{[higher]:30,[lower]:10}),card(2,{[higher]:20,[lower]:30}),card(3,{[lower]:20})];
    assert.deepEqual(ids(cards),[1,2,3]);
    assert.deepEqual(ids(cards,'asc'),[3,2,1]);
    assert.deepEqual(ids([...cards].reverse()),[1,2,3]);
  }
});

test('unrelated releases use the oldest recorded date rather than server priority',()=>{
  const cards=[card(1,{jp:30,cn:10}),card(2,{en:20,tw:15})];
  assert.equal(releaseOf(cards[0]).timestamp,10);
  assert.deepEqual(ids(cards),[2,1]);
  assert.deepEqual(ids(cards,'asc'),[1,2]);
});

test('equal dates keep descending IDs in either direction and every tied card participates in bridges',()=>{
  const cards=[card(1,{jp:20}),card(2,{jp:20}),card(3,{jp:10,cn:30}),card(4,{cn:20})];
  assert.deepEqual(ids(cards),[2,1,3,4]);
  assert.deepEqual(ids(cards,'asc'),[4,3,2,1]);
  const disconnected=[card(1,{jp:10}),card(2,{cn:10})];
  assert.deepEqual(ids(disconnected),[2,1]);
  assert.deepEqual(ids(disconnected,'asc'),[2,1]);
});

test('missing and invalid dates stay last, future dates are sortable and strings are accepted',()=>{
  const cards=[card(9,{}),card(8,{jp:0,cn:-1,en:Infinity,tw:'invalid',kr:null}),
    card(1,{jp:'20'}),card(2,{jp:4102444800000})];
  assert.deepEqual(ids(cards),[2,1,9,8]);
  assert.deepEqual(ids(cards,'asc'),[1,2,9,8]);
  assert.deepEqual(ids([]),[]);
});

test('cache follows only release metadata and invalidates when dates change in place',()=>{
  const cards=bridgeCards(),original=cardReleaseOrder(cards);
  assert.equal(cardReleaseOrder([...cards].reverse().map(c=>({...c,owned:!c.owned}))),original);
  cards[2].releaseDates.cn=Date.UTC(2026,6,20);
  assert.notEqual(cardReleaseOrder(cards),original);
  assert.deepEqual(ids(cards),[10052,2480,2353]);
  assert.deepEqual(ids(bridgeCards(),'desc',original),[2480,2353,10052]);
});

test('a mixed catalogue has a complete stable order independent of input enumeration',()=>{
  let seed=20260917;
  const random=()=>{seed=(Math.imul(seed,1664525)+1013904223)>>>0;return seed;};
  const servers=['jp','cn','en','tw','kr'];
  const cards=Array.from({length:80},(_,i)=>card(i+1,Object.fromEntries(servers.map(s=>[s,random()%4?random()%20+1:null]))));
  const order=cardReleaseOrder(cards);
  for(const direction of ['asc','desc']) {
    const expected=ids(cards,direction,order),rank=new Map(expected.map((id,i)=>[id,i]));
    assert.equal(new Set(expected).size,cards.length);
    assert.deepEqual(ids([...cards].reverse(),direction),expected);
    const compare=comparator('release',direction,order);
    for(const a of cards)for(const b of cards)if(a!==b) assert.equal(Math.sign(compare(a,b)),Math.sign(rank.get(a.id)-rank.get(b.id)));
    const subset=cards.filter(c=>c.id%3===0);
    assert.deepEqual(ids(subset,direction,order),expected.filter(id=>id%3===0));
  }
});
