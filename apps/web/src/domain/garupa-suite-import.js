// garupa-unpacker `src/suiteMaster.ts` writes the decoded SuiteMaster protobuf
// as suite_master.json. Its maps and lists retain their `entries` wrappers.
import {customSkillDurationError,customSkillDurations} from './custom-cards.js';

const isObject = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const mapEntries = value => isObject(value?.entries) ? value.entries : {};
const listEntries = value => Array.isArray(value?.entries) ? value.entries : Object.values(mapEntries(value));
const statKeys = ['performance', 'technique', 'visual'];
const stats = (record, prefix = '') => statKeys.map(key => Number(record?.[prefix + key[0].toUpperCase() + key.slice(1)] ?? record?.[key] ?? 0));
const integer = (value, min, max) => Number.isInteger(Number(value)) && Number(value) >= min && Number(value) <= max;
const requiredStats = (record, label) => {
  const values = stats(record);
  if (!isObject(record) || !statKeys.every(key => record[key] != null) || !values.every(value => integer(value, 0, 999999))) throw new Error(`${label}三维无效`);
  return values;
};
const attribute = value => {
  const raw = String(value ?? '').toLowerCase();
  if (!['powerful', 'cool', 'happy', 'pure'].includes(raw)) throw new Error(`不支持的卡牌属性：${value ?? '空'}`);
  return raw[0].toUpperCase() + raw.slice(1);
};

export function parseGarupaSuiteMaster(input) {
  let master;
  try { master = typeof input === 'string' ? JSON.parse(input) : input; }
  catch { throw new Error('JSON 解析失败，请检查输入内容'); }
  if (Array.isArray(master?.cards) && (master?.clientVersion || master?.dataVersion)) {
    throw new Error('此 JSON 只有卡牌摘要，缺少完整的特训、剧情和技能等级数据');
  }
  const situations = mapEntries(master?.masterCharacterSituationMap);
  if (!Object.keys(situations).length || !isObject(master?.masterSituationSkillMap) || !isObject(master?.masterCharacterInfoMap) || !isObject(master?.masterSkillList) || !isObject(master?.masterSkillActivateEffectList)) {
    throw new Error('JSON 缺少卡牌或关联技能表');
  }
  const characters = mapEntries(master.masterCharacterInfoMap);
  const cards = Object.values(situations).filter(situation => integer(situation?.situationId, 1, 0xffffffff)).map(situation => {
    const characterName = String(characters[situation.characterId]?.characterName || `角色 ${situation.characterId}`);
    const name = String(situation.prefix || characterName);
    return {situationId:Number(situation.situationId),name,characterName,rarity:Number(situation.rarity) || 0,
      attribute:String(situation.attribute || ''),releasedAt:Number(situation.releasedAt) || 0,
      search:`${situation.situationId} ${name} ${characterName}`.toLowerCase()};
  }).sort((a,b) => b.releasedAt-a.releasedAt || b.situationId-a.situationId);
  return {master,cards};
}

// Both Quick commands emit a `cards` array: quickUnpack writes info.json for
// the current release, while quickMaster writes master.json for the latest ten.
export function parseGarupaCardSource(input) {
  let master;
  try { master = typeof input === 'string' ? JSON.parse(input) : input; }
  catch { throw new Error('JSON 解析失败，请检查输入内容'); }
  if (!Array.isArray(master?.cards) || (master?.dataVersion == null && master?.clientVersion == null)) {
    return {...parseGarupaSuiteMaster(master),format:'suite'};
  }
  const ids = new Set();
  const cards = master.cards.map(card => {
    const situationId = Number(card?.situationId);
    if (!isObject(card) || !integer(situationId, 1, 0xffffffff) || ids.has(situationId)) throw new Error('JSON 中的卡牌编号无效或重复');
    ids.add(situationId);
    const characterName = String(card.characterName || `角色 ${card.characterId}`);
    const name = String(card.prefix || characterName);
    return {situationId,name,characterName,rarity:Number(card.rarity) || 0,
      attribute:String(card.attribute || ''),releasedAt:card.releasedAt || '',
      search:`${situationId} ${name} ${characterName}`.toLowerCase()};
  });
  return {master,cards,format:'quick',variant:master.dataVersion != null ? 'quick' : 'quick-master'};
}

// Regular card growth values are fixed by rarity in the current catalog.
// Kirafes and birthday skills identify their different training/episode rows.
const QUICK_BONUS_BY_RARITY = Object.freeze({
  1:[0,100,200],2:[0,150,300],3:[300,200,500],4:[400,250,600],5:[400,250,600],
});
const QUICK_BONUS_BY_SKILL = Object.freeze({
  57:[0,250,600], // birthday 4-star: no training bonus
  70:[0,0,850],   // Kfes 5-star: first episode is fixed at zero
});
function quickBonuses(card) {
  const skillId=Number(card.skill?.skillId);
  const special=Number(card.rarity) === 5 && skillId === 70 || Number(card.rarity) === 4 && skillId === 57;
  const values=special ? QUICK_BONUS_BY_SKILL[skillId] : QUICK_BONUS_BY_RARITY[Number(card.rarity)];
  return {bonuses:values.map((value,index) => ({enabled:index > 0 || value > 0,stats:[value,value,value]})),
    basis:special ? skillId === 70 ? 'Kfes 五星固定值' : '生日四星固定值' : `${card.rarity} 星常规固定值`};
}

function quickSkillById(card, core) {
  const skillId=Number(card.skill?.skillId),skill=core?.skillsFix?.[skillId] ?? core?.skills?.[skillId];
  if (!isObject(skill)) return {reason:`技能 ID ${integer(skillId,1,0xffffffff)?skillId:'缺失'} 尚未在当前游戏数据中找到`};
  const effects=skill.activationEffect?.activateEffectTypes || {},types=Object.keys(effects);
  const supported=new Set(['score','score_perfect','score_only_perfect','score_under_great_half','score_continued_note_judge','score_rate_up_with_perfect']);
  const unsupported=types.filter(type => type.startsWith('score') && !supported.has(type));
  if (unsupported.length) return {reason:`技能 ID ${skillId} 含当前自定义卡牌不支持的效果：${unsupported.join('、')}`};
  const rateup=types.includes('score_rate_up_with_perfect');
  const allowed=customSkillDurations(rateup ? 'rateup' : 'score');
  const durations=Array.isArray(skill.duration) ? skill.duration.map(raw => allowed.find(value => Math.abs(value-Number(raw)) < 0.0001)) : [];
  if (durations.length !== 5 || durations.some(value => value == null)) return {reason:`技能 ID ${skillId} 缺少引擎支持的五级时长`};
  if (card.skill?.duration != null && Math.abs(Number(card.skill.duration)-durations[0]) > 0.0001) return {reason:`技能 ID ${skillId} 的时长与 JSON 不一致`};
  const lastValue=values => Array.isArray(values) ? [...values].reverse().find(value => typeof value === 'number' && Number.isFinite(value)) : typeof values === 'number' ? values : undefined;
  const scored=Object.entries(effects).filter(([type]) => supported.has(type) && type !== 'score_rate_up_with_perfect')
    .map(([type,effect]) => ({type,effect,value:lastValue(effect.activateEffectValue)}))
    .filter(row => Number.isFinite(row.value) && row.effect.activateEffectValueType === 'rate')
    .sort((a,b) => b.value-a.value);
  const primary=scored[0];
  if (!primary || !integer(primary.value,1,1000)) return {reason:`技能 ID ${skillId} 没有可计算的得分加成`};
  const unified=Number(skill.activationEffect?.unificationActivateEffectValue) || primary.value;
  if (!integer(unified,1,1000) || rateup && primary.value !== 100) return {reason:`技能 ID ${skillId} 的加成不在当前引擎支持范围内`};
  const conditionType=skill.activationEffect?.unificationActivateConditionType;
  let conditionAttribute='none';
  if (conditionType && String(conditionType).toLowerCase() !== 'all') {
    try { conditionAttribute=attribute(conditionType); }
    catch { return {reason:`技能 ID ${skillId} 的属性条件尚不支持`}; }
  }
  const conditionBand=Number(skill.activationEffect?.unificationActivateConditionBandId) || 0;
  const skillType=rateup ? 'rateup' : unified !== primary.value || conditionBand || conditionAttribute !== 'none' ? 'unified'
    : types.includes('score_continued_note_judge') ? 'great'
    : types.some(type => ['score_perfect','score_only_perfect','score_under_great_half'].includes(type)) || primary.effect.activateCondition === 'perfect' ? 'perfect' : 'score';
  const lowerScore=skillType === 'great' ? scored.find(row => row.type === 'score')?.value || 0 : 0;
  const ignored=types.filter(type => !type.startsWith('score'));
  return {skill:{skillType,duration:durations[4],durations,skillLevel:5,score:primary.value,unifiedScore:unified,
    lowerScore,conditionAttribute,conditionBand},warnings:ignored.length ? [`技能 ID ${skillId} 的 ${ignored.join('、')} 附加效果不参与当前计算`] : []};
}

export function garupaQuickCardDraft(master, situationId, core) {
  const card = master?.cards?.find(row => Number(row?.situationId) === Number(situationId));
  if (!isObject(card)) throw new Error(`卡牌 ${situationId} 不在 JSON 中`);
  const sourceId = Number(card.situationId),characterId = Number(card.characterId),maxLevel = Number(card.maxLevel);
  if (!integer(characterId, 1, 1000000) || !core?.characters?.[characterId]?.bandId) throw new Error('卡牌角色不在当前游戏数据中，请先更新游戏数据');
  if (!integer(card.rarity, 1, 5)) throw new Error('卡牌稀有度无效');
  if (!integer(maxLevel, 1, 100)) throw new Error('卡牌缺少有效的最高等级');
  const base = requiredStats(card.parameters, '最高等级');
  const {bonuses,basis}=quickBonuses(card);
  const matchedSkill=quickSkillById(card,core);
  const quickDuration=customSkillDurations('score').find(value => Math.abs(value-Number(card.skill?.duration)) < 0.0001);
  const fallbackDuration=quickDuration ?? 7;
  const sourceText = (value,max) => String(value ?? '').slice(0,max);
  const sourceQuick = {variant:master.dataVersion != null ? 'quick' : 'quick-master',
    resourceSetName:sourceText(card.resourceSetName,80),colorCode:sourceText(card.colorCode,20),releasedAt:sourceText(card.releasedAt,32),
    skillId:integer(card.skill?.skillId,1,0xffffffff) ? Number(card.skill.skillId) : null,
    skillName:sourceText(card.skill?.skillName,120),simpleDescription:sourceText(card.skill?.simpleDescription,600),
    description:sourceText(card.skill?.description,2000),duration:quickDuration ?? null,bonusBasis:basis};
  const name = String(card.prefix || card.characterName || `卡牌 ${sourceId}`).trim().slice(0,48);
  const warnings = [`特训和剧情采用 ${basis}；JSON 未单独提供这些数值，其他特殊卡牌请核对。`];
  if (matchedSkill.reason) warnings.push(`${matchedSkill.reason}；卡牌已停用，技能暂以 0% 占位，请手动补全。`);
  else warnings.push(...matchedSkill.warnings);
  if (matchedSkill.reason && quickDuration == null) warnings.push('原始 1 级技能时长不受当前引擎支持，暂以 7 秒占位。');
  const skill=matchedSkill.skill || {skillType:'score',duration:fallbackDuration,durations:Array(5).fill(fallbackDuration),skillLevel:1,
    score:0,unifiedScore:0,lowerScore:0,conditionAttribute:'none',conditionBand:0};
  return {draft:{enabled:!!matchedSkill.skill,name,character:characterId,attribute:attribute(card.attribute),rarity:Number(card.rarity),
    stats:base,mastery:0,...skill,image:'',
    notes:`JSON 导入 · 原卡牌 ID ${sourceId}${matchedSkill.reason?' · 技能待补全':''}${sourceQuick.skillName ? ` · ${sourceQuick.skillName}` : ''}`.slice(0,240),
    sourceSituationId:sourceId,sourceQuick,
    advanced:{current:maxLevel,levels:[{level:maxLevel,stats:base}],bonuses}},
    warnings,match:{matched:!!matchedSkill.skill,reason:matchedSkill.reason||''}};
}

export function garupaSuiteCardDraft(master, situationId, core) {
  const situation = mapEntries(master?.masterCharacterSituationMap)[String(situationId)];
  if (!isObject(situation)) throw new Error(`卡牌 ${situationId} 不在 SuiteMaster 中`);
  const sourceId = Number(situation.situationId);
  if (sourceId !== Number(situationId)) throw new Error('卡牌编号与 SuiteMaster 索引不一致');
  const characterId = Number(situation.characterId);
  if (!integer(sourceId, 1, 0xffffffff) || !integer(characterId, 1, 1000000) || !core?.characters?.[characterId]?.bandId) {
    throw new Error('卡牌角色不在当前游戏数据中，请先更新游戏数据');
  }
  if (!integer(situation.rarity, 1, 5)) throw new Error('卡牌稀有度无效');
  const levels = Object.values(isObject(situation.parameterMap) ? situation.parameterMap : {}).map(row => ({
    level:Number(row?.level),stats:requiredStats(row, `${row?.level ?? '?'} 级`),
  })).sort((a,b) => a.level-b.level);
  if (!levels.length || levels.some(row => !integer(row.level, 1, 100)) || new Set(levels.map(row => row.level)).size !== levels.length) throw new Error('卡牌等级三维不完整');
  const training = situation.training;
  const trainingStats = training ? stats(training, 'training') : [0,0,0];
  if (!trainingStats.every(value => integer(value, 0, 999999))) throw new Error('特训三维无效');
  const episodeRecords = listEntries(situation.episodes);
  if (episodeRecords.length > 2) throw new Error('卡牌剧情超过两段，当前自定义卡牌格式无法表示');
  const episodes = [0,1].map(index => episodeRecords[index] ? {
    enabled:true,stats:statKeys.map(key => Number(episodeRecords[index][`append${key[0].toUpperCase()+key.slice(1)}`] ?? 0)),
  } : {enabled:false,stats:[0,0,0]});
  if (episodes.some(episode => !episode.stats.every(value => integer(value, 0, 999999)))) throw new Error('剧情三维无效');

  const skillRef = mapEntries(master.masterSituationSkillMap)[String(situation.situationSkillId)];
  const skillId = Number(skillRef?.skillId);
  if (!integer(skillId, 1, 0xffffffff)) throw new Error('找不到卡牌关联的技能定义');
  const skillRows = listEntries(master.masterSkillList).filter(row => Number(row.skillId) === skillId);
  const durations = [1,2,3,4,5].map(level => Number(skillRows.find(row => Number(row.skillLevel) === level)?.duration));
  if (durations.some(value => !Number.isFinite(value))) throw new Error('缺少五个技能等级的持续时间');
  const effects = listEntries(master.masterSkillActivateEffectList).filter(row => Number(row.skillId) === skillId);
  const highest = effects.filter(row => Number(row.skillLevel) === 5);
  const types = new Set(highest.map(row => String(row.activateEffectType || '')));
  const supported = new Set(['score','score_perfect','score_under_great_half','score_continued_note_judge','score_rate_up_with_perfect']);
  const unsupported = [...types].filter(type => type.startsWith('score') && !supported.has(type));
  if (unsupported.length) throw new Error(`当前计算器尚不支持此技能类型：${unsupported.join('、')}`);
  const rateup = types.has('score_rate_up_with_perfect');
  const primary = highest.filter(row => supported.has(row.activateEffectType) && row.activateEffectType !== 'score_rate_up_with_perfect')
    .sort((a,b) => Number(b.activateEffectValue)-Number(a.activateEffectValue))[0];
  if (!primary || !integer(primary.activateEffectValue, 0, 1000) || primary.activateEffectValueType && primary.activateEffectValueType !== 'rate') {
    throw new Error('技能没有可用于计算的得分加成');
  }
  const skillType = rateup ? 'rateup' : Number(primary.unificationActivateEffectValue) > 0 ? 'unified'
    : types.has('score_continued_note_judge') ? 'great'
    : types.has('score_perfect') || types.has('score_under_great_half') || primary.activateCondition === 'perfect' ? 'perfect' : 'score';
  // Protobuf float32 durations such as 5.6 may decode as 5.599999904632568.
  durations.forEach((value,index) => {
    const supported = customSkillDurations(skillType).find(candidate => Math.abs(candidate-value) < 0.0001);
    if (supported !== undefined) durations[index] = supported;
  });
  for (const duration of durations) {
    const message = customSkillDurationError(duration, skillType);
    if (message) throw new Error(`技能时长 ${duration} 秒暂不支持：${message}`);
  }
  if (rateup && Number(primary.activateEffectValue) !== 100) throw new Error('递增技能的基础加成不是当前计算器要求的 100%');
  const unifiedScore = Number(primary.unificationActivateEffectValue) || Number(primary.activateEffectValue);
  if (!integer(unifiedScore, 0, 1000)) throw new Error('技能条件加成无效');
  const conditionAttribute = primary.unificationActivateConditionType && primary.unificationActivateConditionType !== 'all'
    ? attribute(primary.unificationActivateConditionType) : 'none';
  const conditionBand = Number(primary.unificationActivateConditionBandId) || 0;
  const maxLevel = levels.at(-1).level;
  const name = String(situation.prefix || mapEntries(master.masterCharacterInfoMap)[String(characterId)]?.characterName || `卡牌 ${sourceId}`).trim().slice(0,48);
  const ignored = [...types].filter(type => type && !type.startsWith('score'));
  if (types.has('score_under_great_half')) ignored.push('GREAT 以下得分减半');
  if (types.has('score_continued_note_judge')) ignored.push('GREAT 以下降档');
  const onceEffects = listEntries(master.masterSkillOnceEffectList).filter(row => Number(row.skillId) === skillId && Number(row.skillLevel) === 5);
  ignored.push(...onceEffects.map(row => String(row.onceEffectType || '一次性效果')));
  const lowerScore = skillType === 'great' ? Number(highest.find(row => row.activateEffectType === 'score')?.activateEffectValue) || 0 : 0;
  return {draft:{enabled:true,name,character:characterId,attribute:attribute(situation.attribute),rarity:Number(situation.rarity),
    stats:[...levels.at(-1).stats],mastery:0,skillType,duration:durations[4],durations,skillLevel:5,
    score:Number(primary.activateEffectValue),unifiedScore,lowerScore,conditionAttribute,conditionBand,
    image:'',notes:`JSON 导入 · 原卡牌 ID ${sourceId}`,sourceSituationId:sourceId,sourceSkillId:skillId,
    advanced:{current:maxLevel,levels,bonuses:[{enabled:!!training,stats:trainingStats},...episodes]}},
    warnings:ignored.length ? [`${ignored.join('、')}等附加技能效果不参与当前计算`] : []};
}
