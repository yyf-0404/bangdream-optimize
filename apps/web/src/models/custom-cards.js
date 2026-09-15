// Kept separate from the official card list. IDs stay below signed 32-bit range
// and away from the u32::MAX IDs used for cooperative teammates.
export const CUSTOM_CARD_ID_BASE = 1_000_000_000;
export const CUSTOM_CARD_ID_MAX = 1_999_999_999;
export const CUSTOM_SKILL_DURATIONS = Object.freeze([3,3.5,4,4.5,5,5.5,5.6,5.7,6,6.2,6.4,6.5,6.8,7,7.2,7.5,8]);
export const CUSTOM_RATEUP_DURATIONS = Object.freeze([5,5.5,6,6.5,7]);
export const customSkillDurations = type => type === 'rateup' ? CUSTOM_RATEUP_DURATIONS : CUSTOM_SKILL_DURATIONS;
export function customSkillDurationError(value, type) {
  if (value == null || String(value).trim() === '') return '请填写技能持续时间';
  const allowed = customSkillDurations(type);
  return allowed.includes(Number(value)) ? '' : `支持的时长：${allowed.join('、')} 秒`;
}
export function stepCustomSkillDuration(value, type, direction) {
  const allowed = customSkillDurations(type), number = Number(value);
  if (value == null || String(value).trim() === '' || !Number.isFinite(number)) return allowed[0];
  return direction > 0
    ? allowed.find(duration => duration > number) ?? allowed.at(-1)
    : [...allowed].reverse().find(duration => duration < number) ?? allowed[0];
}
const keys = ['performance', 'technique', 'visual'];
const attrs = ['powerful', 'cool', 'happy', 'pure'];
const stat = values => Object.fromEntries(keys.map((key, i) => [key, Number(values[i])]));
const vector = value => keys.map(key => Number(value?.[key] ?? 0));
const clone = value => structuredClone(value);
const percent = value => Number((value * 100).toFixed(6));
const isObject = value => value && typeof value === 'object' && !Array.isArray(value);
export const isCustomCardId = id => Number.isInteger(Number(id)) && Number(id) > CUSTOM_CARD_ID_BASE && Number(id) <= CUSTOM_CARD_ID_MAX;
export const customCardLabel = id => `C-${String(Number(id) - CUSTOM_CARD_ID_BASE).padStart(3, '0')}`;

export function validateCustomCard(entry, id) {
  const fail = message => { throw new Error(`自定义卡牌 ${id}：${message}`); };
  const integer = (v, min, max) => Number.isInteger(v) && v >= min && v <= max;
  const finite = (v, min, max) => typeof v === 'number' && Number.isFinite(v) && v >= min && v <= max;
  const validStat = v => isObject(v) && keys.every(k => integer(v[k], 0, 999999));
  if (!isCustomCardId(id) || String(Number(id)) !== String(id) || !isObject(entry)) fail('编号或资料无效');
  const d = entry.definition, g = entry.growth, s = d?.skill;
  if (typeof entry.uid !== 'string' || !entry.uid || entry.uid.length > 128) fail('缺少唯一标识');
  if (typeof entry.enabled !== 'boolean' || !isObject(d) || d.cardId !== Number(id)) fail('卡牌编号不一致');
  if (!integer(d.characterId, 1, 1000000) || !integer(d.bandId, 1, 1000000) || !integer(d.rarity, 1, 5) || !attrs.includes(d.attribute)) fail('角色、乐队、稀有度或属性无效');
  if (!isObject(d.levelStats) || !Object.keys(d.levelStats).length || Object.entries(d.levelStats).some(([level, v]) => !integer(Number(level), 1, 100) || !validStat(v))) fail('等级三维无效');
  if (!validStat(d.trainingStat) || !Array.isArray(d.episodeStats) || d.episodeStats.length !== 2 || !d.episodeStats.every(validStat)) fail('特训或剧情三维无效');
  if (!isObject(g) || !integer(g.level, 1, 100) || !d.levelStats[g.level] || !integer(g.skillLevel, 1, 5) || !integer(g.limitBreakRank, 0, 4) || typeof g.training !== 'boolean' || !Array.isArray(g.episodes) || g.episodes.length !== 2 || g.episodes.some(v => typeof v !== 'boolean')) fail('养成参数无效');
  if (!s || !Array.isArray(s.durations) || s.durations.length !== 5 || !s.durations.every(v => finite(v, .1, 60)) || !finite(s.scoreUp?.default, 0, 10) || typeof s.rateup !== 'boolean') fail('技能参数无效');
  if (!s.durations.every(v => customSkillDurations(s.rateup ? 'rateup' : 'score').includes(v))) fail('技能时长不在当前引擎支持的范围内');
  if (s.rateup && s.scoreUp.default !== 1) fail('递增技能的起始加成固定为 100%');
  const u = s.scoreUp;
  if (u.unificationActivateEffectValue != null && !finite(u.unificationActivateEffectValue, 0, 10)) fail('条件加成无效');
  if (u.unificationActivateConditionBandId != null && !integer(u.unificationActivateConditionBandId, 1, 1000000)) fail('技能乐队条件无效');
  if (u.unificationActivateConditionType != null && !attrs.includes(u.unificationActivateConditionType)) fail('技能属性条件无效');
  if (entry.editor != null) {
    const e = entry.editor;
    if (!isObject(e) || ['name','image','notes','skillType'].some(k => e[k] != null && typeof e[k] !== 'string') || (e.fixed != null && typeof e.fixed !== 'boolean') || (e.updated != null && !finite(e.updated, 0, Number.MAX_SAFE_INTEGER)) || (e.lowerScore != null && !finite(e.lowerScore, 0, 1000))) fail('编辑资料格式无效');
  }
  return entry;
}

export function normalizeCustomCards(cards = {}) {
  if (!isObject(cards)) throw new Error('自定义卡牌列表格式无效');
  const result = {}, uids = new Set();
  for (const [id, raw] of Object.entries(cards)) {
    const entry = clone(raw);
    validateCustomCard(entry, id);
    if (uids.has(entry.uid)) throw new Error('自定义卡牌唯一标识重复');
    uids.add(entry.uid);result[id] = entry;
  }
  return result;
}

export function nextCustomCardId(player) {
  if (player.nextCustomCardId != null && (!Number.isSafeInteger(player.nextCustomCardId) || player.nextCustomCardId < 0 || player.nextCustomCardId > CUSTOM_CARD_ID_MAX + 1)) throw new Error('自定义卡牌编号计数无效');
  return Math.max(CUSTOM_CARD_ID_BASE + 1, Number(player.nextCustomCardId) || 0, ...Object.keys(player.customCards || {}).map(id => Number(id) + 1));
}

export function customCardEntry(draft, core, {id, uid = globalThis.crypto.randomUUID()} = {}) {
  if (!isCustomCardId(id)) throw new Error('自定义卡牌编号已用尽');
  const character = core?.characters?.[draft.character];
  if (!character?.bandId) throw new Error('请选择当前游戏数据中的角色');
  const a = draft.advanced, zero = stat([0, 0, 0]);
  // The editor has one effective duration. Repeat it to keep the existing
  // engine/storage schema; legacy drafts without it still deserialize normally.
  const scoreUp = {default: Number(draft.score) / 100};
  if (draft.skillType === 'unified') {
    scoreUp.unificationActivateEffectValue = Number(draft.unifiedScore) / 100;
    if (Number(draft.conditionBand)) scoreUp.unificationActivateConditionBandId = Number(draft.conditionBand);
    if (draft.conditionAttribute !== 'none') scoreUp.unificationActivateConditionType = draft.conditionAttribute.toLowerCase();
  }
  const entry = {uid, enabled: draft.enabled !== false,
    definition: {cardId:id,characterId:Number(draft.character),bandId:Number(character.bandId),rarity:Number(draft.rarity),attribute:draft.attribute.toLowerCase(),
      levelStats: a ? Object.fromEntries(a.levels.map(r => [r.level, stat(r.stats)])) : {60:stat(draft.stats)},
      trainingStat: a ? stat(a.bonuses[0].stats) : {...zero},episodeStats:a ? a.bonuses.slice(1).map(b => stat(b.stats)) : [{...zero},{...zero}],
      skill:{durations:draft.duration !== undefined ? Array(5).fill(Number(draft.duration)) : draft.durations.map(Number),scoreUp,rateup:draft.skillType === 'rateup'}},
    growth:{level:a ? Number(a.current) : 60,training:a ? a.bonuses[0].enabled : true,episodes:a ? a.bonuses.slice(1).map(b=>b.enabled) : [true,true],limitBreakRank:Number(draft.mastery),skillLevel:draft.duration !== undefined ? 5 : Number(draft.skillLevel),illustTrainingStatus:true},
    editor:{name:String(draft.name).trim().slice(0,48),image:safeCustomImage(draft.image),notes:String(draft.notes || '').slice(0,240),skillType:draft.skillType,lowerScore:Number(draft.lowerScore)||0,fixed:!a,updated:Number(draft.updated)||Date.now()}};
  return validateCustomCard(entry, id);
}

export function customCardDraft(entry) {
  const {definition:d,growth:g,editor:e={}}=entry,s=d.skill.scoreUp;
  return {id:d.cardId,uid:entry.uid,enabled:entry.enabled,name:e.name||customCardLabel(d.cardId),character:d.characterId,attribute:d.attribute[0].toUpperCase()+d.attribute.slice(1),rarity:d.rarity,
    stats:vector(d.levelStats[g.level]),mastery:g.limitBreakRank,skillType:d.skill.rateup?'rateup':s.unificationActivateEffectValue!=null?'unified':['score','perfect','great'].includes(e.skillType)?e.skillType:'score',
    duration:d.skill.durations[g.skillLevel-1],score:percent(s.default),unifiedScore:percent(s.unificationActivateEffectValue??s.default),lowerScore:e.lowerScore??0,
    conditionBand:s.unificationActivateConditionBandId??0,conditionAttribute:s.unificationActivateConditionType? s.unificationActivateConditionType[0].toUpperCase()+s.unificationActivateConditionType.slice(1):'none',
    image:safeCustomImage(e.image),notes:e.notes||'',updated:e.updated||0,
    advanced:e.fixed?null:{current:g.level,levels:Object.entries(d.levelStats).map(([level,v])=>({level:Number(level),stats:vector(v)})),bonuses:[d.trainingStat,...d.episodeStats].map((v,i)=>({stats:vector(v),enabled:i===0?g.training:g.episodes[i-1]}))}};
}

export function safeCustomImage(value) {
  if (typeof value !== 'string' || value.length > 2_800_000) return '';
  if (/^data:image\/(png|jpeg|webp);base64,[A-Za-z0-9+/=]+$/.test(value)) return value;
  try {const url=new URL(value);return url.protocol==='https:'&&url.hostname==='bestdori.com'?url.href:'';} catch {return '';}
}

// Keep large uploaded artwork out of cache keys while invalidating presentation changes.
export function customCardPresentationKey(entry) {
  const {image = '', ...editor} = entry.editor || {};
  let hash = 2166136261;
  for (let i = 0; i < image.length; i++) hash = Math.imul(hash ^ image.charCodeAt(i), 16777619);
  return {...editor, image: `${image.length}:${hash >>> 0}`};
}

export function mergeCustomCards(base, incoming = {}, incomingNextId) {
  const result=clone(base.customCards||{}), imported=normalizeCustomCards(incoming), remap={};
  const reservedUntil=nextCustomCardId(base);
  let next=reservedUntil;
  if(incomingNextId!=null)nextCustomCardId({nextCustomCardId:incomingNextId});
  for(const [rawId,entry] of Object.entries(imported)) {
    const same=Object.entries(result).find(([,old])=>old.uid===entry.uid);
    let id=same?Number(same[0]):Number(rawId);
    if(!same && (result[id] || id < reservedUntil)) {while(result[next])next++;id=next++;}
    if(!isCustomCardId(id))throw new Error('自定义卡牌编号已用尽');
    remap[rawId]=id;entry.definition.cardId=id;result[id]=entry;
    next=Math.max(next,id+1);
  }
  return {customCards:result,nextCustomCardId:Math.max(next,Number(incomingNextId)||0),remap};
}

export function customSkillInfo(entry, level = entry.growth.skillLevel) {
  const c=customCardDraft(entry),duration=entry.definition.skill.durations[level-1],condition=[c.conditionAttribute!=='none'?c.conditionAttribute:'',c.conditionBand?`乐队 ${c.conditionBand}`:''].filter(Boolean).join(' 与 ');
  let score=c.score,description=`${duration} 秒内，得分提升 ${score}%。`,suffix='',category='分数提升';
  if(c.skillType==='perfect'){suffix='P';category='PERFECT 条件加分';description=`${duration} 秒内，PERFECT 时得分提升 ${score}%。`;}
  if(c.skillType==='great'){suffix='G';category='GREAT 以下降档';description=`${duration} 秒内得分提升 ${score}%，出现 GREAT 以下后降为 ${c.lowerScore}%。`;}
  if(c.skillType==='unified'){score=c.unifiedScore;category='同属性／乐队条件加分';description=condition?`${duration} 秒内得分提升 ${c.score}%；全队满足 ${condition} 时提升至 ${score}%。`:`${duration} 秒内得分提升 ${score}%，属性与乐队均不限制。`;}
  if(c.skillType==='rateup'){suffix='+0.5*P';category='PERFECT 递增加分';description=`${duration} 秒内从 ${score}% 起，每个 PERFECT 额外提升 0.5%，上限 ${score+50}%。`;}
  return {id:null,short:category,category,score,value:`${score}%`,notation:String(score)+suffix,extra:[],description,duration,effects:[]};
}
