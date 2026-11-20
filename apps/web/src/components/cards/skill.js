import {gameText} from '../../shared/preferences.js';
import {customSkillInfo} from '../../domain/custom-cards.js';

// Categories follow the user-supplied tsugu drawCardIconSkill reference.
// https://github.com/yyf-0404/tsugu-bangdream-bot/blob/medley/backend/src/components/skill.ts
const percent=value=>Number(Number(value).toFixed(4));
const rateUpEffect = 'score_rate_up_with_perfect';

export const skillBonusFilterLabels = Object.freeze({
  '150-plus': '150% 及以上',
  '130-145': '130%～145%',
  '100-125': '100%～125%',
  'under-100': '低于 100%',
});

function skillBonusGroup(bonus) {
  if (bonus >= 150) return '150-plus';
  if (bonus >= 130) return '130-145';
  if (bonus >= 100) return '100-125';
  return 'under-100';
}

export function normalizeSkillBonusFilter(values) {
  const groups = new Set();
  for (const value of values ?? []) {
    if (Object.hasOwn(skillBonusFilterLabels, value)) groups.add(value);
    else if (value === 'rateup') groups.add('100-125');
    else if (value != null && value !== '' && Number.isFinite(Number(value))) {
      groups.add(skillBonusGroup(Number(value)));
    }
  }
  return groups;
}

function scoreEffectValues(effects) {
  return Object.entries(effects)
    .filter(([type]) => type.startsWith('score'))
    .map(([, effect]) => effect?.activateEffectValue);
}

export function cardSkillFilterValue(card) {
  const skill = card.skillRecord;
  if (!skill) return undefined;
  const activation = skill.activationEffect;
  const effects = activation?.activateEffectTypes ?? {};
  if (Object.hasOwn(effects, rateUpEffect)) return '100-125';
  const bonuses = scoreEffectValues(effects).flat().filter(Number.isFinite);
  if (Number.isFinite(activation?.unificationActivateEffectValue)) {
    bonuses.push(activation.unificationActivateEffectValue);
  }
  return skillBonusGroup(Math.max(0, ...bonuses));
}

function conditionalScore(base,unified,suffix,teamCards,bandId,attribute){
 const constrained=bandId!=null||attribute!=null;
 if(!constrained)return {score:unified,value:`${unified}%`,notation:`${unified}${suffix}`};
 if(!Array.isArray(teamCards))return {score:Math.max(base,unified),value:`${base}% / ${unified}%`,notation:`${base}/${unified}${suffix}`};
 const matched=teamCards.length===5&&teamCards.every(card=>card&&!card.unknown
  &&(bandId==null||Number(card.band)===Number(bandId))
  &&(attribute==null||String(card.attribute).toLowerCase()===String(attribute).toLowerCase()));
 const score=matched?unified:base;
 return {score,value:`${score}%`,notation:`${score}${suffix}`};
}
export function cardSkillInfo(card){
 const teamCards=card.skillTeamCards;
 if(card.custom){
  const info=customSkillInfo(card.customConfig,card.skill),birthday=Number(card.customConfig?.editor?.sourceSkillId ?? card.customConfig?.editor?.sourceQuick?.skillId)===57;
  if(birthday)return {...info,short:'B',category:'B 型 · 生日卡加分',notation:`${info.score}B`};
  const up=card.customConfig?.definition?.skill?.scoreUp;
  if(up?.unificationActivateEffectValue==null)return info;
  return {...info,...conditionalScore(percent(Number(up.default)*100),percent(Number(up.unificationActivateEffectValue)*100),'',teamCards,
   up.unificationActivateConditionBandId,up.unificationActivateConditionType)};
 }
 const id=card.skillId,s=card.skillRecord;
 if(!s)return {id:null,short:'未知',category:'技能资料缺失',value:'—',score:null,notation:'—',extra:[],description:'当前资料没有这张卡牌的技能记录。',duration:null,effects:[]};
 const level=Math.max(0,Math.min(4,(Number(card.skill)||5)-1));
 const atLevel=values=>Array.isArray(values)?values[level]??[...values].reverse().find(v=>typeof v==='number')??null:values??null;
 const effects=s.activationEffect?.activateEffectTypes||{},types=new Set(Object.keys(effects));
 if(effects.score?.activateCondition==='perfect')types.add('score_perfect');
 if(s.onceEffect?.onceEffectType)types.add(s.onceEffect.onceEffectType);
 const scores=scoreEffectValues(effects).map(atLevel).filter(Number.isFinite);
 const base=scores.length?Math.max(...scores):0,unified=s.activationEffect?.unificationActivateEffectValue;
 const value=typeof unified==='number'?Math.max(base,unified):base;
 let suffix='';
 if(types.has('score_continued_note_judge'))suffix='G';
 else if(types.has('score_over_life'))suffix=types.has('score_under_life')?'L':'/';
 else if(types.has('score_under_great_half')||types.has('score_perfect'))suffix='P';
 else if(types.has(rateUpEffect))suffix='+0.5*P';
 let short='分数',category='分数提升';
 if(types.has('score_continued_note_judge')){short='G';category='G 型 · GREAT 以下后降档';}
 else if(types.has('score_over_life')){short=types.has('score_under_life')?'L':'满血';category=types.has('score_under_life')?'L 型 · 按生命值分档':'生命值条件加分';}
 else if(types.has('score_under_great_half')||types.has('score_perfect')){short='P';category='P 型 · PERFECT 条件加分';}
 else if(types.has(rateUpEffect)){short='递增';category='PERFECT 递增加分';}
 else if(s.activationEffect?.unificationActivateEffectValue!=null){short='组队';category='同属性／乐队条件加分';}
 const extras=['judge','life','damage'].filter(t=>types.has(t));
 const names={judge:'判定',life:'回复',damage:'伤害'};
 if(extras.length){const extra=extras.map(t=>names[t]).join('／');category=scores.length?extra+' + '+category:extra;if(short==='分数')short=extra;}
 const birthday=Number(id)===57;
 if(birthday){suffix='B';short='B';category='B 型 · 生日卡加分';}
 const duration=atLevel(s.duration),once=atLevel(s.onceEffect?.onceEffectValue);
 const replacements=s.onceEffect?.onceEffectType?[once,duration]:[duration];
 const template=gameText(s.description,'技能资料缺失');
 const description=template.replace(/\{(\d+)\}/g,(match,n)=>replacements[Number(n)]??'—');
 const display=typeof unified==='number'?conditionalScore(base,unified,suffix,teamCards,
  s.activationEffect?.unificationActivateConditionBandId,s.activationEffect?.unificationActivateConditionType)
  :{score:value,value:value?`${value}%`:'—',notation:value?`${value}${suffix}`:''};
 return {id,short,category,...display,description,duration,effects:[...types],extra:birthday?[]:extras};
}
