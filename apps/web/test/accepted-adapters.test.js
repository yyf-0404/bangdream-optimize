import assert from 'node:assert/strict';
import test from 'node:test';
import {equipmentAvailable} from '../src/ui/equipment.js';
import {allowedLiveVariants} from '../src/models/player-settings.js';
import {bonusApplication} from '../src/ui/activity.js';

test('equipment rejects missing required levels while keeping the two optional common items',()=>{
  const player={areaItem:{1:{level:1},59:{level:0},72:{level:0}}};
  assert.equal(equipmentAvailable({areaItemIds:['1','59','72']},player),true);
  assert.equal(equipmentAvailable({areaItemIds:['1','2']},player),false);
  assert.equal(equipmentAvailable(undefined,player),false);
});
test('bonus editor activity and performance choices respect calculation capabilities',()=>{
  assert.deepEqual(allowedLiveVariants('challenge','ptEvaluate'),['solo','challenge_cp']);
  assert.deepEqual(allowedLiveVariants('challenge','ptMaximize'),['solo','cooperative','challenge_cp']);
  assert.deepEqual(allowedLiveVariants('medley','ptEvaluate'),['medley']);
  assert.equal(bonusApplication('challenge','ptEvaluate','solo'),'point');
  assert.equal(bonusApplication('challenge','ptEvaluate','challenge_cp'),'stat');
  assert.equal(bonusApplication('medley','ptEvaluate','medley'),'stat');
});
