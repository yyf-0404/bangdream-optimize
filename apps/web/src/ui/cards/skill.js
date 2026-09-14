import {gameText} from '../preferences.js';

// Categories follow the user-supplied tsugu drawCardIconSkill reference.
// https://github.com/yyf-0404/tsugu-bangdream-bot/blob/medley/backend/src/components/skill.ts
export function cardSkillInfo(card){
 const id=card.skillId,s=card.skillRecord;
 if(!s)return {id:null,short:'未知',category:'技能资料缺失',value:'—',score:null,notation:'—',extra:[],description:'当前资料没有这张卡牌的技能记录。',duration:null,effects:[]};
 const level=Math.max(0,Math.min(4,(Number(card.skill)||5)-1));
 const atLevel=values=>Array.isArray(values)?values[level]??[...values].reverse().find(v=>typeof v==='number')??null:values??null;
 const effects=s.activationEffect?.activateEffectTypes||{},types=new Set(Object.keys(effects));
 if(effects.score?.activateCondition==='perfect')types.add('score_perfect');
 if(s.onceEffect?.onceEffectType)types.add(s.onceEffect.onceEffectType);
 const scores=Object.entries(effects).filter(([type])=>type.startsWith('score')) .map(([,e])=>atLevel(e.activateEffectValue)).filter(v=>typeof v==='number'&&Number.isFinite(v));
 if(typeof s.activationEffect?.unificationActivateEffectValue==='number')scores.push(s.activationEffect.unificationActivateEffectValue);
 const value=scores.length?Math.max(...scores):0;
 let suffix='';
 if(types.has('score_continued_note_judge'))suffix='G';
 else if(types.has('score_over_life'))suffix=types.has('score_under_life')?'L':'/';
 else if(types.has('score_under_great_half')||types.has('score_perfect'))suffix='P';
 else if(types.has('score_rate_up_with_perfect'))suffix='+0.5*P';
 let short='分数',category='分数提升';
 if(types.has('score_continued_note_judge')){short='G';category='G 型 · GREAT 以下后降档';}
 else if(types.has('score_over_life')){short=types.has('score_under_life')?'L':'满血';category=types.has('score_under_life')?'L 型 · 按生命值分档':'生命值条件加分';}
 else if(types.has('score_under_great_half')||types.has('score_perfect')){short='P';category='P 型 · PERFECT 条件加分';}
 else if(types.has('score_rate_up_with_perfect')){short='递增';category='PERFECT 递增加分';}
 else if(s.activationEffect?.unificationActivateEffectValue!=null){short='组队';category='同属性／乐队条件加分';}
 const extras=['judge','life','damage'].filter(t=>types.has(t));
 const names={judge:'判定',life:'回复',damage:'伤害'};
 if(extras.length){const extra=extras.map(t=>names[t]).join('／');category=scores.length?extra+' + '+category:extra;if(short==='分数')short=extra;}
 const duration=atLevel(s.duration),once=atLevel(s.onceEffect?.onceEffectValue);
 const replacements=s.onceEffect?.onceEffectType?[once,duration]:[duration];
 const template=gameText(s.description,'技能资料缺失');
 const description=template.replace(/\{(\d+)\}/g,(match,n)=>replacements[Number(n)]??'—');
 return {id,short,category,value:value?value+'%':'—',score:value,notation:value?String(value)+suffix:'',description,duration,effects:[...types],extra:extras};
}
