import test from 'node:test';
import assert from 'node:assert/strict';
import {stepCustomSkillDuration,customSkillDurationError,customCardPresentationKey,customCardEntry,customCardDraft,customCardLabel,customCardReferences,removeCustomCardReferences,mergeCustomCards,nextCustomCardId,normalizeCustomCards} from '../src/domain/custom-cards.js';
import {cardModel,catalogModels} from '../src/components/cards/model.js';
import {cardStatValues} from '../src/components/cards/stats.js';
import {cardSkillInfo} from '../src/components/cards/skill.js';
import {validatePtEvaluateTeamSelection} from '../src/domain/pt-evaluate-validation.js';
import {teamChoiceReason} from '../src/pages/activity/team-rules.js';
import {createCompactProfileCodec} from '../src/data/compact-profile.js';
import {createPlayerModel} from '../src/domain/player.js';
import {createViewAdapters} from '../src/app/view-adapters.js';
import {parseGarupaCardSource,parseGarupaSuiteMaster,garupaSuiteCardDraft,garupaQuickCardDraft} from '../src/domain/garupa-suite-import.js';
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
test('conditional skill labels show both bonuses outside a team and the active bonus inside it',()=>{
 const matching=Array.from({length:5},()=>({band:1,attribute:'cool'}));
 const mixed=matching.map((card,index)=>index===4?{...card,attribute:'pure'}:card);
 const mixedBand=matching.map((card,index)=>index===4?{...card,band:2}:card);
 const custom=cardModel(core,player(make({skillType:'unified',conditionAttribute:'Cool',conditionBand:1})),id);
 assert.equal(cardSkillInfo(custom).notation,'130/150');
 assert.equal(cardSkillInfo(custom).value,'130% / 150%');
 assert.equal(cardSkillInfo({...custom,skillTeamCards:matching}).notation,'150');
 assert.equal(cardSkillInfo({...custom,skillTeamCards:matching}).value,'150%');
 assert.equal(cardSkillInfo({...custom,skillTeamCards:mixed}).notation,'130');
 assert.equal(cardSkillInfo({...custom,skillTeamCards:mixedBand}).notation,'130');
 assert.equal(cardSkillInfo({...custom,skillTeamCards:matching.slice(0,4)}).notation,'130');
 const bandOnly=cardModel(core,player(make({skillType:'unified',conditionAttribute:'none',conditionBand:1})),id);
 const attributeOnly=cardModel(core,player(make({skillType:'unified',conditionAttribute:'Cool',conditionBand:0})),id);
 assert.equal(cardSkillInfo({...bandOnly,skillTeamCards:mixed}).notation,'150');
 assert.equal(cardSkillInfo({...attributeOnly,skillTeamCards:mixedBand}).notation,'150');

 const game={skill:5,skillId:70,skillRecord:{duration:[7,7,7,7,7],activationEffect:{
  unificationActivateEffectValue:150,unificationActivateConditionBandId:1,unificationActivateConditionType:'cool',
  activateEffectTypes:{score:{activateEffectValue:[90,100,110,120,130]}}}}};
 assert.equal(cardSkillInfo(game).notation,'130/150');
 assert.equal(cardSkillInfo({...game,skillTeamCards:matching}).notation,'150');
 assert.equal(cardSkillInfo({...game,skillTeamCards:mixed}).notation,'130');
 assert.equal(cardSkillInfo({...game,skillTeamCards:mixedBand}).notation,'130');
 assert.equal(cardSkillInfo({...game,skillTeamCards:matching.slice(0,4)}).notation,'130');
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
test('result text and image use the calculation snapshot for a deleted custom card',()=>{
 const snapshot=player(make({image:'data:image/png;base64,aGVsbG8='}));
 let presentation;
 const adapters=createViewAdapters({elements:{resultSummary:{}},renderResultSummaryView:(_host,_result,deps)=>{presentation=deps;},
  readPlayer:()=>player(undefined),getCore:()=>core,cardLabel:cardId=>`游戏卡牌 ${cardId}`,
  cardName:cardId=>`游戏卡牌 ${cardId}`,cardRarity:()=>1,cardCharacterId:()=>0,cardAttribute:()=> 'pure',cardIconUrls:()=>[]});
 adapters.renderResultSummary({}, {diagnostic:{player:snapshot}});
 assert.equal(presentation.cardName(id),'试算');
 assert.equal(presentation.cardLabel(id),'试算 · C-001');
 assert.equal(presentation.cardRarity(id),5);
 assert.equal(presentation.cardAttribute(id),'cool');
 assert.equal(presentation.cardCharacterId(id),1);
 assert.equal(presentation.cardIconUrls(id)[0],'data:image/png;base64,aGVsbG8=');
 assert.equal(presentation.cardLabel(1),'游戏卡牌 1');
});
test('deleting a custom card clears team slots and event bonuses without changing other cards',()=>{
 const p=player(make());
 p.ptEvaluate={teams:[[id,2,0,0,0],[3,id,0,0,0]]};
 p.eventOverrides={0:{members:[{situationId:id,percent:20},{situationId:1,percent:30}]}};
 p.eventPresets={5:{members:[{cardId:id,percent:40},{situationId:2,percent:50}]}};
 assert.deepEqual(customCardReferences(p,id),{teamSlots:2,eventBonuses:2});
 assert.deepEqual(removeCustomCardReferences(p,id),{teamSlots:2,eventBonuses:2});
 assert.deepEqual(p.ptEvaluate.teams,[[0,2,0,0,0],[3,0,0,0,0]]);
 assert.deepEqual(p.eventOverrides[0].members,[{situationId:1,percent:30}]);
 assert.deepEqual(p.eventPresets[5].members,[{situationId:2,percent:50}]);
 assert.deepEqual(customCardReferences(p,id),{teamSlots:0,eventBonuses:0});
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

function suiteMasterFixture(effectType='score') {
 const skillId=9701;
 return {
  masterCharacterSituationMap:{entries:{9901:{situationId:9901,characterId:1,rarity:5,attribute:'cool',prefix:'解包卡牌',
   situationSkillId:9902,releasedAt:'1780000000000',parameterMap:{'1':{level:1,performance:100,technique:200,visual:300},'60':{level:60,performance:1000,technique:2000,visual:3000}},
   training:{trainingPerformance:10,trainingTechnique:20,trainingVisual:30},episodes:{entries:[
    {episodeId:1,appendPerformance:1,appendTechnique:2,appendVisual:3},
    {episodeId:2,appendPerformance:4,appendTechnique:5,appendVisual:6}]}}}},
  masterCharacterInfoMap:{entries:{1:{characterId:1,characterName:'Kasumi',bandId:1}}},
  masterSituationSkillMap:{entries:{9902:{situationSkillId:9902,skillId}}},
  masterSkillList:{entries:[1,2,3,4,5].map((skillLevel,index)=>({skillId,skillLevel,duration:[5,5.5,5.599999904632568,6.5,7][index]}))},
  masterSkillActivateEffectList:{entries:[1,2,3,4,5].map(skillLevel=>({skillId,skillLevel,seq:1,activateEffectType:effectType,activateEffectValueType:'rate',activateEffectValue:110+skillLevel*5,activateCondition:'good'}))},
 };
}

test('garupa SuiteMaster import joins card, growth, episode and skill tables without losing levels',()=>{
 const source=suiteMasterFixture();
 const catalog=parseGarupaSuiteMaster(JSON.stringify(source));
 assert.equal(catalog.cards[0].situationId,9901);
 assert.match(catalog.cards[0].search,/kasumi/);
 const {draft:imported,warnings}=garupaSuiteCardDraft(catalog.master,9901,core);
 assert.deepEqual(warnings,[]);
 assert.equal(imported.sourceSituationId,9901);
 assert.deepEqual(imported.advanced.levels,[{level:1,stats:[100,200,300]},{level:60,stats:[1000,2000,3000]}]);
 assert.deepEqual(imported.advanced.bonuses.map(item=>item.stats),[[10,20,30],[1,2,3],[4,5,6]]);
 assert.deepEqual(imported.durations,[5,5.5,5.6,6.5,7]);
 assert.equal(imported.score,135);
 const entry=customCardEntry(imported,core,{id,uid:'garupa-import'});
 assert.deepEqual(entry.definition.skill.durations,imported.durations);
 assert.equal(entry.editor.sourceSituationId,9901);
 assert.equal(entry.editor.sourceSkillId,9701);
 assert.equal(cardStatValues(cardModel(core,player(entry),id)).total,6081);
 const toggled=customCardEntry({...customCardDraft(entry),enabled:false},core,{id,uid:entry.uid});
 assert.deepEqual(toggled.definition.skill.durations,imported.durations);
 assert.equal(toggled.editor.sourceSituationId,9901);
 const changed=customCardEntry({...customCardDraft(entry),duration:6},core,{id,uid:entry.uid});
 assert.deepEqual(changed.definition.skill.durations,[6,6,6,6,6]);
});

test('Quick info and quick-master cards import by skill ID with matching growth defaults',()=>{
 const quickCard={situationId:9901,characterId:1,characterName:'Kasumi',rarity:5,attribute:'cool',prefix:'Quick Kfes',
  resourceSetName:'res001001',maxLevel:60,parameters:{performance:1000,technique:2000,visual:3000},
  skill:{skillId:70,skillName:'Kfes Skill',simpleDescription:'PERFECT 得分提升 150%',description:'5 秒内得分提升 150%',duration:5}};
 const game={...core,cards:{1329:{characterId:1,rarity:5,attribute:'cool',levelLimit:60,skillId:70,
  stat:{training:{performance:0,technique:0,visual:0},episodes:[{performance:0,technique:0,visual:0},{performance:850,technique:850,visual:850}]}}},
  skills:{70:{duration:[5,5.5,6,6.5,7],activationEffect:{activateEffectTypes:{score_under_great_half:{activateEffectValue:[150,150,150,150,null],activateEffectValueType:'rate',activateCondition:'perfect'}}}}}};
 for(const wrapper of [{dataVersion:'8.0.0',cards:[quickCard],musics:[]},{clientVersion:'8.0.0',cards:[quickCard],events:[],songs:[]}]){
  const catalog=parseGarupaCardSource(JSON.stringify(wrapper));
  assert.equal(catalog.format,'quick');assert.equal(catalog.cards[0].situationId,9901);
  const {draft:imported,warnings,match}=garupaQuickCardDraft(catalog.master,9901,game);
  assert.equal(imported.enabled,true);assert.equal(imported.skillType,'perfect');assert.equal(imported.score,150);
  assert.deepEqual(match,{matched:true,reason:''});
  assert.deepEqual(imported.durations,[5,5.5,6,6.5,7]);assert.equal(imported.skillLevel,5);
  assert.deepEqual(imported.advanced.bonuses.map(item=>item.stats),[[0,0,0],[0,0,0],[850,850,850]]);
  assert.deepEqual(imported.advanced.bonuses.map(item=>item.enabled),[false,true,true]);
  assert.ok(warnings.some(value=>value.includes('Kfes 五星固定值')));
  const entry=customCardEntry(imported,game,{id,uid:'quick-import'});
  assert.equal(entry.editor.sourceQuick.skillId,70);
  assert.equal(entry.definition.episodeStats[1].performance,850);
  assert.equal(cardStatValues(cardModel(game,player(entry),id)).total,8550);
  assert.equal(customCardDraft(entry).sourceQuick.simpleDescription,quickCard.skill.simpleDescription);
 }
});

test('Quick card stays disabled when its skill ID is unavailable and keeps its source description',()=>{
 const quick={clientVersion:'8.0.0',cards:[{situationId:9902,characterId:1,characterName:'Kasumi',rarity:5,attribute:'cool',prefix:'Unresolved',
  maxLevel:60,parameters:{performance:1000,technique:2000,visual:3000},skill:{skillId:999,skillName:'New Skill',description:'原始技能说明',duration:5}}]};
 const catalog=parseGarupaCardSource(quick),{draft:imported,warnings,match}=garupaQuickCardDraft(catalog.master,9902,core);
 assert.equal(imported.enabled,false);assert.equal(imported.score,0);assert.equal(imported.duration,5);
 assert.equal(match.matched,false);assert.match(match.reason,/技能 ID 999/);
 assert.deepEqual(imported.advanced.bonuses.map(item=>item.stats),[[400,400,400],[250,250,250],[600,600,600]]);
 assert.ok(warnings.some(value=>value.includes('技能 ID 999')));
 const entry=customCardEntry(imported,core,{id,uid:'quick-unresolved'});
 assert.equal(customCardDraft(entry).sourceQuick.description,'原始技能说明');
 assert.throws(()=>parseGarupaCardSource({clientVersion:'8.0.0',cards:[quick.cards[0],quick.cards[0]]}),/编号无效或重复/);
});

test('Quick birthday cards use their fixed growth exception and skill ID 57',()=>{
 const birthday={dataVersion:'8.0.0',cards:[{situationId:9903,characterId:1,characterName:'Kasumi',rarity:4,attribute:'cool',prefix:'Birthday',
  maxLevel:60,parameters:{performance:4000,technique:5000,visual:6000},skill:{skillId:57,skillName:'Birthday Skill',duration:5}}]};
 const game={...core,skills:{57:{duration:[5,5.5,6,6.5,7],activationEffect:{activateEffectTypes:{
  score_continued_note_judge:{activateEffectValue:[100,100,100,100,100],activateEffectValueType:'rate',activateCondition:'perfect'},
  score:{activateEffectValue:[40,40,40,40,40],activateEffectValueType:'rate',activateCondition:'good'},
  damage:{activateEffectValue:[0,0,0,0,0],activateEffectValueType:'rate',activateCondition:'none'}}}}}};
 const {draft:imported,warnings}=garupaQuickCardDraft(birthday,9903,game);
 assert.equal(imported.enabled,true);assert.equal(imported.skillType,'great');assert.equal(imported.score,100);assert.equal(imported.lowerScore,40);
 assert.deepEqual(imported.advanced.bonuses.map(item=>item.stats),[[0,0,0],[250,250,250],[600,600,600]]);
 assert.deepEqual(imported.advanced.bonuses.map(item=>item.enabled),[false,true,true]);
 assert.ok(warnings.some(value=>value.includes('生日四星固定值')));
 assert.ok(warnings.some(value=>value.includes('damage')));
 const custom=cardModel(core,player(customCardEntry(imported,core,{id,uid:'quick-birthday'})),id);
 assert.equal(cardSkillInfo(custom).short,'B');
 assert.equal(cardSkillInfo(custom).notation,'100B');
 const gameCard={skillId:57,skill:5,skillRecord:game.skills[57]};
 assert.equal(cardSkillInfo(gameCard).short,'B');
 assert.equal(cardSkillInfo(gameCard).notation,'100B');
 assert.deepEqual(cardSkillInfo(gameCard).extra,[]);
 assert.ok(cardSkillInfo(gameCard).effects.includes('damage'));
});

test('SuiteMaster birthday skill ID survives import and uses the B label',()=>{
 const source=suiteMasterFixture('score_continued_note_judge');
 source.masterSituationSkillMap.entries[9902].skillId=57;
 source.masterSkillList.entries.forEach(row=>{row.skillId=57;});
 source.masterSkillActivateEffectList.entries.forEach(row=>{row.skillId=57;});
 const imported=garupaSuiteCardDraft(source,9901,core).draft;
 assert.equal(imported.sourceSkillId,57);
 const entry=customCardEntry(imported,core,{id,uid:'suite-birthday'});
 assert.equal(customCardDraft(entry).sourceSkillId,57);
 assert.equal(cardSkillInfo(cardModel(core,player(entry),id)).notation,'135B');
});

test('Quick source details survive compact profile export and import',async()=>{
 const quick={clientVersion:'8.0.0',cards:[{situationId:9904,characterId:1,characterName:'Kasumi',rarity:4,attribute:'cool',prefix:'Archived Quick',
  maxLevel:60,parameters:{performance:4000,technique:5000,visual:6000},skill:{skillId:999,skillName:'Source Skill',description:'原始技能描述',duration:5}}]};
 const imported=garupaQuickCardDraft(quick,9904,core).draft,entry=customCardEntry(imported,core,{id,uid:'quick-compact'});
 const codec=createCompactProfileCodec({normalizedPlayer:p=>p}),payload=codec.buildCompactProfilePayload(player(entry));
 const compressed=await codec.compressProfilePayload(payload);
 const decoded=await codec.parseCompactProfileExport(JSON.stringify({v:compressed.version,t:compressed.type,d:compressed.data}));
 const restored=codec.compactProfileToPlayer(decoded,{cardList:{},customCards:{}});
 assert.deepEqual(restored.customCards[id].editor.sourceQuick,entry.editor.sourceQuick);
});

test('garupa import rejects summaries and unsupported score mechanics rather than inventing data',()=>{
 assert.throws(()=>parseGarupaSuiteMaster({cards:[{situationId:1}]}),/JSON 缺少卡牌或关联技能表/);
 assert.throws(()=>parseGarupaSuiteMaster({clientVersion:'1.0.0',cards:[{situationId:1}]}),/缺少完整的特训、剧情和技能等级数据/);
 assert.throws(()=>parseGarupaSuiteMaster({dataVersion:'1.0.0',cards:[{situationId:1}]}),/缺少完整的特训、剧情和技能等级数据/);
 const source=suiteMasterFixture('score_over_life');
 assert.throws(()=>garupaSuiteCardDraft(source,9901,core),/尚不支持/);
 source.masterSkillActivateEffectList.entries=[];
 assert.throws(()=>garupaSuiteCardDraft(source,9901,core),/没有可用于计算/);
 assert.throws(()=>garupaSuiteCardDraft(suiteMasterFixture(),9901,{characters:{}}),/更新游戏数据/);
});

test('garupa import maps unified and rate-up skills into current calculation templates',()=>{
 const unified=suiteMasterFixture();
 unified.masterSkillActivateEffectList.entries.forEach(row=>{row.unificationActivateEffectValue=150;row.unificationActivateConditionType='cool';row.unificationActivateConditionBandId=1;});
 const unifiedDraft=garupaSuiteCardDraft(unified,9901,core).draft;
 assert.equal(unifiedDraft.skillType,'unified');
 assert.equal(unifiedDraft.unifiedScore,150);
 assert.equal(unifiedDraft.conditionAttribute,'Cool');
 assert.equal(unifiedDraft.conditionBand,1);
 const rateup=suiteMasterFixture();
 rateup.masterSkillList.entries[2].duration=6;
 rateup.masterSkillActivateEffectList.entries.forEach(row=>row.activateEffectValue=100);
 rateup.masterSkillActivateEffectList.entries.push(...[1,2,3,4,5].map(skillLevel=>({skillId:9701,skillLevel,activateEffectType:'score_rate_up_with_perfect',activateEffectValue:0})));
 const rateupDraft=garupaSuiteCardDraft(rateup,9901,core).draft;
 assert.equal(rateupDraft.skillType,'rateup');
 assert.equal(rateupDraft.score,100);
 assert.equal(customCardEntry(rateupDraft,core,{id,uid:'rateup-import'}).definition.skill.rateup,true);
 const great=suiteMasterFixture('score_continued_note_judge');
 great.masterSkillActivateEffectList.entries.push({skillId:9701,skillLevel:5,seq:2,activateEffectType:'score',activateEffectValueType:'rate',activateEffectValue:55});
 great.masterSkillOnceEffectList={entries:[{skillId:9701,skillLevel:5,onceEffectType:'life'}]};
 const greatImport=garupaSuiteCardDraft(great,9901,core);
 assert.equal(greatImport.draft.skillType,'great');
 assert.equal(greatImport.draft.lowerScore,55);
 assert.ok(greatImport.warnings.some(warning=>warning.includes('life')));
});
