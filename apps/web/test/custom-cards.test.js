import test from 'node:test';
import assert from 'node:assert/strict';
import {stepCustomSkillDuration,customSkillDurationError,customCardPresentationKey,customCardEntry,customCardDraft,customCardLabel,mergeCustomCards,nextCustomCardId,normalizeCustomCards} from '../src/models/custom-cards.js';
import {cardModel,catalogModels} from '../src/ui/cards/model.js';
import {cardStatValues} from '../src/ui/cards/stats.js';
import {cardSkillInfo} from '../src/ui/cards/skill.js';
import {validatePtEvaluateTeamSelection} from '../src/models/pt-evaluate-validation.js';
import {teamChoiceReason} from '../src/ui/team-rules.js';
import {createCompactProfileCodec} from '../src/data/compact-profile.js';
import {createPlayerModel} from '../src/models/player.js';
globalThis.window={setTimeout:(fn,delay)=>{const timer=setTimeout(fn,delay);timer.unref();return timer;}};
const core={characters:Object.fromEntries([1,2,3,4,5].map(id=>[id,{bandId:1,characterName:['Kasumi','','','角色 '+id]}])),cards:{1:{characterId:1,rarity:1,attribute:'pure'}}};
const draft={name:'试算',character:1,rarity:5,attribute:'Cool',stats:[11000,12000,13000],mastery:2,duration:7,score:130,skillType:'score',conditionAttribute:'none',conditionBand:0,unifiedScore:150,lowerScore:110,enabled:true,notes:'测试',image:'',advanced:null};
const id=1_000_000_001;
const make=(patch={},n=id,uid='test-uid')=>customCardEntry({...draft,...patch},core,{id:n,uid});
const player=entry=>({customCards:{[id]:entry},cardList:{},areaItem:{},characterBouns:{},nextCustomCardId:id+1});

test('fixed stats add mastery once; advanced stats honor selected training and episodes',()=>{
 const entry=make(),p=player(entry),card=cardModel(core,p,id);
 assert.equal(customCardLabel(id),'C-001');assert.equal(cardStatValues(card).total,37500);
 assert.equal(cardSkillInfo(card).notation,'130');assert.equal(cardSkillInfo(card).duration,7);
 const advanced={current:30,levels:[{level:30,stats:[100,200,300]},{level:60,stats:[500,600,700]}],bonuses:[{enabled:false,stats:[900,900,900]},{enabled:true,stats:[10,20,30]},{enabled:false,stats:[40,50,60]}]};
 const adv=make({advanced,mastery:0}),model=cardModel(core,player(adv),id);
 assert.deepEqual(cardStatValues(model),{performance:110,technique:220,visual:330,total:660});
 assert.deepEqual(customCardEntry(customCardDraft(adv),core,{id,uid:adv.uid}),adv);
 assert.equal(catalogModels(core,p).length,1);assert.ok(!catalogModels(core,p).some(c=>c.id===id));
});
test('unrestricted conditions are independently omitted; all five skill templates survive serialization',()=>{
 for(const skillType of ['score','perfect','great','unified','rateup']){
  const entry=make({skillType,score:skillType==='rateup'?100:130}),copy=make(customCardDraft(entry));
  assert.deepEqual(copy.definition,entry.definition);assert.equal(customCardDraft(copy).skillType,skillType);
 }
 const both=make({skillType:'unified'}).definition.skill.scoreUp;
 assert.deepEqual(both,{default:1.3,unificationActivateEffectValue:1.5});
 const attr=make({skillType:'unified',conditionAttribute:'Cool'}).definition.skill.scoreUp;
 assert.equal(attr.unificationActivateConditionType,'cool');assert.equal(attr.unificationActivateConditionBandId,undefined);
 const band=make({skillType:'unified',conditionBand:1}).definition.skill.scoreUp;
 assert.equal(band.unificationActivateConditionBandId,1);assert.equal(band.unificationActivateConditionType,undefined);
});
test('import merges UUIDs, remaps colliding IDs, and never reuses deleted IDs',()=>{
 const base=player(make()),changed=make({score:140},id+10),foreign=make({},id,'foreign');
 const result=mergeCustomCards(base,{[id]:foreign,[id+10]:changed});
 assert.equal(result.customCards[id].definition.skill.scoreUp.default,1.4);
 assert.equal(result.remap[id],id+1);assert.equal(result.remap[id+10],id);
 assert.equal(result.customCards[id+1].uid,'foreign');assert.equal(result.nextCustomCardId,id+2);
 assert.equal(nextCustomCardId({customCards:{},nextCustomCardId:result.nextCustomCardId}),id+2);
 const deleted=mergeCustomCards({customCards:{},nextCustomCardId:id+10},{[id]:foreign});
 assert.equal(deleted.remap[id],id+10);assert.equal(deleted.nextCustomCardId,id+11);
 assert.throws(()=>nextCustomCardId({nextCustomCardId:Infinity}),/计数/);
 assert.throws(()=>normalizeCustomCards({[id]:make(),[id+1]:make({},id+1)}),/唯一标识重复/);
});
test('compact export round trip retains custom data, disabled status, images and the allocation counter',async()=>{
 const codec=createCompactProfileCodec({normalizedPlayer:p=>p}),entry=make({enabled:false,image:'data:image/png;base64,aGVsbG8='}),p=player(entry);
 const payload=codec.buildCompactProfilePayload(p),compressed=await codec.compressProfilePayload(payload);
 assert.equal(compressed.type,'gz+b64');
 const decoded=await codec.parseCompactProfileExport(JSON.stringify({v:compressed.version,t:compressed.type,d:compressed.data}));
 const imported=codec.compactProfileToPlayer(decoded,{cardList:{},customCards:{}});
 assert.deepEqual(imported.customCards,p.customCards);assert.equal(imported.nextCustomCardId,id+1);
 const empty=codec.buildCompactProfilePayload({...p,customCards:{}});assert.equal(empty.x.nextId,id+1);
 const legacy=codec.buildCompactProfilePayload({cardList:{},areaItem:{},characterBouns:{}});assert.equal((await codec.compressProfilePayload(legacy)).type,'bit1+b64');
});
test('specified teams accept custom IDs, but reject disabled or duplicate characters across card sources',()=>{
 const p=player(make());p.cardList={2:{},3:{},4:{},5:{}};
 const request={liveVariant:'solo',teams:[{cardIds:[id,2,3,4,5],captainCardId:3}]};
 validatePtEvaluateTeamSelection(p,request,Number);
 p.customCards[id].enabled=false;
 assert.throws(()=>validatePtEvaluateTeamSelection(p,request,Number),/已停用/);
 assert.match(teamChoiceReason(cardModel(core,p,id),[[0,2,3,4,5]],0,0,Number),/停用|启用/);
 p.customCards[id].enabled=true;p.customCards[id].definition.characterId=2;
 assert.throws(()=>validatePtEvaluateTeamSelection(p,request,Number),/不同角色/);
});
test('normalization preserves canonical custom cards and rejects malformed growth without partial saves',()=>{
 const {normalizedPlayer}=createPlayerModel({normalizedActivityMode:x=>x,normalizedCalculationMode:x=>x});
 const p=player(make()),normalized=normalizedPlayer(p);
 assert.deepEqual(normalized.customCards,p.customCards);assert.equal(normalized.nextCustomCardId,id+1);
 p.customCards[id].growth.skillLevel=6;assert.throws(()=>normalizedPlayer(p),/养成参数/);
});
test('result card models use the supplied historical definition after live edits and deletion',()=>{
 const live=player(make()),snapshot=structuredClone(live);
 live.customCards[id].definition.levelStats[60].performance=1;delete live.customCards[id];
 const result=cardModel(core,snapshot,id,snapshot.customCards[id].growth);
 assert.equal(result.custom,true);assert.equal(result.title,'试算');assert.equal(cardStatValues(result).total,37500);
});

test('custom metadata changes invalidate presentation caches without embedding artwork in keys',()=>{
 const entry=make({score:113,unifiedScore:137});
 assert.equal(customCardDraft(entry).score,113);
 const original=customCardPresentationKey(entry);
 entry.editor.name='改名';assert.notDeepEqual(customCardPresentationKey(entry),original);
 const renamed=customCardPresentationKey(entry);
 entry.editor.image='data:image/png;base64,'+'a'.repeat(100000);
 assert.notDeepEqual(customCardPresentationKey(entry),renamed);
 assert.ok(JSON.stringify(customCardPresentationKey(entry)).length<1000);
 entry.editor.name=42;assert.throws(()=>normalizeCustomCards({[id]:entry}),/编辑资料/);
});

test('custom skills accept only existing engine durations and the fixed rate-up base',()=>{
 assert.throws(()=>make({duration:9}),/技能时长/);
 assert.throws(()=>make({skillType:'rateup',score:130}),/固定为 100%/);
 assert.throws(()=>make({skillType:'rateup',score:100,duration:7.5}),/技能时长/);
 const ramp=make({skillType:'rateup',score:100});assert.equal(ramp.definition.skill.scoreUp.default,1);
});


test('single-duration editing preserves the effective time of older level-based custom cards',()=>{
 const legacy=make({duration:undefined,skillLevel:2,durations:[5,5.5,6,6.5,7]});
 const edited=customCardDraft(legacy);
 assert.equal(edited.duration,5.5);
 assert.equal(edited.skillLevel,undefined);
 const saved=make(edited);
 assert.deepEqual(saved.definition.skill.durations,[5.5,5.5,5.5,5.5,5.5]);
 assert.equal(cardSkillInfo(cardModel(core,player(saved),id)).duration,5.5);
 assert.equal(cardSkillInfo(cardModel(core,player(legacy),id)).duration,5.5);
});

test('duration input rejects empty and unsupported values, including after a template change',()=>{
 assert.match(customSkillDurationError('', 'score'),/填写/);
 assert.match(customSkillDurationError('6.1','score'),/支持的时长/);
 assert.equal(customSkillDurationError('6.5','score'),'');
 assert.equal(customSkillDurationError('7.5','score'),'');
 assert.match(customSkillDurationError('7.5','rateup'),/支持的时长/);
 for(const value of ['NaN','Infinity','-1','0','8.1'])assert.ok(customSkillDurationError(value,'score'));
 assert.throws(()=>make({duration:6.1}),/技能时长/);
});


test('duration arrows move between supported slots and clamp at boundaries',()=>{
 assert.equal(stepCustomSkillDuration(6,'score',1),6.2);
 assert.equal(stepCustomSkillDuration(6.5,'score',1),6.8);
 assert.equal(stepCustomSkillDuration(6.5,'score',-1),6.4);
 assert.equal(stepCustomSkillDuration(6.1,'score',1),6.2);
 assert.equal(stepCustomSkillDuration(6.1,'score',-1),6);
 assert.equal(stepCustomSkillDuration(6,'rateup',1),6.5);
 assert.equal(stepCustomSkillDuration(6,'rateup',-1),5.5);
 assert.equal(stepCustomSkillDuration(7.5,'rateup',-1),7);
 assert.equal(stepCustomSkillDuration(7,'rateup',1),7);
 assert.equal(stepCustomSkillDuration(3,'score',-1),3);
 assert.equal(stepCustomSkillDuration(8,'score',1),8);
 assert.equal(stepCustomSkillDuration('','rateup',1),5);
});
