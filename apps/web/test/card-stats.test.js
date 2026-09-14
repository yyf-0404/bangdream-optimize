import test from 'node:test';
import assert from 'node:assert/strict';
import {cardStatValues} from '../src/ui/cards/stats.js';

const card={level:60,rarity:4,capabilities:[[false,true],[1,1]],record:{stat:{
  1:{performance:100,technique:200,visual:300},60:{performance:1000,technique:2000,visual:3000},
  training:{performance:50,technique:60,visual:70},episodes:[{performance:10,technique:20,visual:30},{performance:40,technique:50,visual:60}],
}},growth:{trained:true,illustTrained:false,mastery:2,episodes:[true,false]}};

test('card-only power includes selected growth components and excludes artwork/skill changes',()=>{
 assert.deepEqual(cardStatValues(card),{performance:1460,technique:2480,visual:3500,total:7440});
 assert.deepEqual(cardStatValues({...card,skill:1,growth:{...card.growth,illustTrained:true}}),cardStatValues(card));
 assert.deepEqual(cardStatValues({...card,growth:{trained:false,mastery:0,episodes:[false,false]}}),{performance:1000,technique:2000,visual:3000,total:6000});
 assert.deepEqual(cardStatValues({...card,level:1,growth:{...card.growth,episodes:[true,true]}}),{performance:600,technique:730,visual:860,total:2190});
});

test('missing card or exact level data shows unavailable, not another level or NaN',()=>{
 assert.equal(cardStatValues({...card,level:2}),null);
 assert.equal(cardStatValues({...card,record:undefined}),null);
 assert.equal(cardStatValues({...card,level:0}),null);
 assert.equal(cardStatValues({...card,record:{stat:{60:{performance:1,technique:2}}}}),null);
});
