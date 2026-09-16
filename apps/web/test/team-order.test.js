import assert from 'node:assert/strict';
import test from 'node:test';
import { teamOrderPresentation, formatOrderProbability, renderActivationOrder } from '../src/ui/team-order.js';

const base = {teamCardIds: [10,20,30,40,50], captainCardId:30};
test('max-score display uses recommended slots, keeping activations separate', () => {
  const order = {recommendedTeamCardIds:[50,10,30,20,40],skillOrderCardIds:[10,30,40,20,50]};
  const value = teamOrderPresentation({...base, teamOrder:order}, true);
  assert.deepEqual(value.ids, order.recommendedTeamCardIds);
  assert.deepEqual(value.activation.skillOrderCardIds, order.skillOrderCardIds);
  assert.equal(value.legacyOrder, false);
});
test('PT recommendations preserve captain and specified teams keep original slots', () => {
  const recommendation = [40,50,30,20,10];
  assert.deepEqual(teamOrderPresentation({...base,recommendedTeamCardIds:recommendation}, false).ids, recommendation);
  const specified = teamOrderPresentation({...base,detailedScore:true}, false);
  assert.deepEqual(specified.ids, base.teamCardIds);
  assert.match(specified.note, /指定卡位/);
});
test('cooperative positions are displayed with a central captain without skill probabilities', () => {
  const value = teamOrderPresentation({...base,captainCardId:10,liveVariant:'cooperative'}, false);
  assert.equal(value.ids[2],10);
  assert.equal(value.activation,undefined);
  assert.match(value.note,/其余卡位等效/);
});
test('legacy and malformed recommendations never invent probability or misplace captain', () => {
  for (const ids of [undefined,[10,20,20,40,50],[10,20,99,40,50],[10,30,20,40,50]]) {
    const value=teamOrderPresentation({...base,teamOrder:{recommendedTeamCardIds:ids}},true);
    assert.deepEqual(value.ids,base.teamCardIds);
    assert.equal(value.legacyOrder,true);
    assert.equal(value.activation,undefined);
  }
});
test('individual sequence probability is distinct from aggregate maximum probability', () => {
  assert.equal(formatOrderProbability(19,1024),'19/1024');
  assert.equal(formatOrderProbability(4,1024),'1/256');
  assert.equal(formatOrderProbability(1024,1024),'1/1');
  assert.equal(formatOrderProbability(72,1024),'9/128');
  assert.equal(formatOrderProbability(0,1024),'0/1');
  assert.equal(formatOrderProbability(1,0),'—');
  assert.equal(formatOrderProbability(1,Infinity),'—');
});

// A small DOM stand-in keeps this display regression independent of a browser.
test('displayed probability sums every optimal order, rather than just the illustrated sequence', () => {
  const previous=globalThis.document;
  class Node {
    constructor(tag){this.tagName=tag;this.children=[];this.text='';}
    set textContent(text){this.text=String(text);this.children=[];}
    get textContent(){return this.text+this.children.map(c=>c.textContent??c).join('');}
    append(...nodes){this.children.push(...nodes);}
    setAttribute(){}
  }
  globalThis.document={createElement:tag=>new Node(tag)};
  try {
    const order={recommendedTeamCardIds:[10,20,30,40,50],skillOrderCardIds:[20,10,40,50,30],skillOrderPathCount:17,maxScorePathCount:96,totalPathCount:1024,optimalOrderCount:6};
    const view=renderActivationOrder(order,30,{cardLabel:String});
    const heading=view.children[0].textContent;
    assert.match(heading,/最高分概率 3\/32/);
    assert.doesNotMatch(heading,/17\/1024/);
    assert.match(view.textContent,/共 6 种触发顺序/);
    assert.ok(!view.children.some(node=>node.tagName==='details'));
    const all=renderActivationOrder({...order,maxScorePathCount:1024,optimalOrderCount:96},30,{cardLabel:String});
    assert.match(all.children[0].textContent,/最高分概率 1\/1/);
  } finally {
    if(previous===undefined)delete globalThis.document;else globalThis.document=previous;
  }
});
