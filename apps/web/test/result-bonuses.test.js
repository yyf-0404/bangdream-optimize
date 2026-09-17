import test from 'node:test';
import assert from 'node:assert/strict';
import {equipmentBonusText,resultCharacterBonuses} from '../src/models/result-bonuses.js';
import {createAreaItemHelpers} from '../src/domain/area.js';
import {createGameMeta} from '../src/domain/meta.js';

test('equipment summary distinguishes percent units and magazine stat, without false zero on missing data',()=>{
  assert.equal(equipmentBonusText({performance:27,technique:27,visual:27}),'综合力 +27%');
  assert.equal(equipmentBonusText({performance:18,technique:0,visual:0}),'演出 +18%');
  assert.equal(equipmentBonusText({performance:0,technique:0,visual:0}),'综合力 +0%');
  assert.equal(equipmentBonusText(undefined),'加成数据缺失');
});
test('all result shapes collect only team characters, deduplicating and preserving custom character identity',()=>{
  const player={customCards:{1000001:{definition:{characterId:2}}},characterBouns:{1:{potential:{performance:.03,technique:.02,visual:.01},characterTask:{performance:.02,technique:.04,visual:.06}},3:{potential:{performance:1}}}};
  const resolve=id=>id===10||id===11?1:undefined;
  for(const result of [{songs:[{teamCardIds:[10,11]},{teamCardIds:[1000001]}]},
    {medley:{teams:[{teamCardIds:[10,1000001]}]}},
    {scoreMode:{mode:'manual'},team:{teamCardIds:[10,1000001]}},
    [{teamCardIds:[10,1000001]}]]){
    const bonuses=resultCharacterBonuses(result,player,resolve);
    assert.deepEqual(bonuses.map(c=>c.id),[1,2]);
    assert.equal(bonuses[0].potential.technique,.02);
    assert.equal(bonuses[0].characterTask.visual,.06);
    assert.equal(bonuses[0].total.performance,.05);
    assert.equal(bonuses[1].total.visual,0);
  }
});
test('equipment bonuses use snapshot server and levels, including the common shell/coffee maximum',()=>{
  const rates={1:[10,null,null,20,null],2:[30,null,null,40,null]};
  const make=target=>({targetBandIds:[],targetAttributes:target,performance:rates,technique:rates,visual:rates});
  const areaItems={59:make(['powerful','cool','pure','happy']),72:make(['powerful','cool','pure','happy']),80:make([])};
  const meta=createGameMeta({getCore:()=>({areaItems}),serverIndex:()=>0});
  const helper=createAreaItemHelpers({getAreaItemRecords:()=>areaItems,recordWithFix:meta.recordWithFix,serverScopedValue:meta.serverScopedValue});
  const player={server:'cn',areaItem:{59:{level:1},72:{level:2}}};
  const groups=helper.areaItemGroups(player);
  assert.equal(groups.find(g=>g.category==='attribute').rate.performance,40);
  assert.equal(helper.areaItemGroups({...player,server:'jp'}).find(g=>g.category==='attribute').rate.performance,30);
  assert.equal(meta.serverScopedValue([1,2,3,4,5]),1);
});
