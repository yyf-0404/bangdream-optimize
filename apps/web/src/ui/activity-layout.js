import {designFragment} from './approved/templates.js';
import {mountDesignControls,hydrateDesignIcons} from './design-controls.js';
import {openBonusEditor} from './bonus-editor.js';
import { designIcon } from './fidelity.js';
import { assetImage, attributeIconUrls, characterIconUrls, cardIconUrls, starIconUrls } from '../assets/index.js';
import { gameText } from './preferences.js';
import { attributeNames } from './cards/rules.js';
import { mountEventSelection, eventTypeNames } from './event-selection.js';
const el=(tag,cls,text)=>{const n=document.createElement(tag);n.className=cls||'';if(text!==undefined)n.textContent=text;return n;};

export function mountActivityLayout({elements,getPlayer,getCore,getProfileId,writePlayer,renderForms,eventSnapshot,parameters,changeCustomType,breaks}){
 const root=document.querySelector('#aurora-soft-study'),q=s=>root.querySelector(s),target=q('.activity-selector-card');
 const approved=designFragment('activity-target');hydrateDesignIcons(approved);
 const legacy=q('.activity-selector-content');legacy.hidden=true;root.append(legacy);
 const fieldset=elements.calculationMode,original=[...fieldset.querySelectorAll('input')];
 const replacement=approved.querySelector('fieldset');
 for(const input of replacement.querySelectorAll('input'))input.replaceWith(original.find(n=>n.value===input.value));
 fieldset.className='bo-target-group';fieldset.replaceChildren(...replacement.childNodes);replacement.replaceWith(fieldset);
 target.className='activity-selector-card bo-section';target.replaceChildren(...approved.childNodes);
 const context=target.querySelector('.bo-section-heading>span');
 const eventSelection=mountEventSelection({root,target,elements,getPlayer,getProfileId,eventSnapshot,changeCustomType});
 target.querySelector('.bo-note').textContent='活动名称、类型、头图与加成随预设联动；歌曲可独立选择。';
 target.querySelector('#bo-advanced').remove();
 parameters.className='calc-settings bo-subsection';parameters.id='bo-calculation-settings';target.append(parameters);
 const bonus=q('.activity-bonus-card');bonus.id='bo-bonuses';bonus.querySelector('h3').innerHTML=designIcon('bonus')+'活动加成';
 const edit=el('button','pb-link');edit.type='button';bonus.querySelector('.bo-section-heading').append(edit);
 bonus.querySelector('.config-content').hidden=true;const summary=el('div','pb-summary');bonus.append(summary);summary.after(breaks);
 edit.onclick=()=>editBonuses();
 const controls=mountDesignControls({root,parameters,legacy:[elements.scoreRangeControls,elements.ptMaximizeControls,elements.ptEvaluateControls],getPlayer,writePlayer,renderForms});
 const liveSettings=controls.live;
 for(const [selector,id,name,mark] of [['.activity-song-card','bo-songs','活动歌曲','song'],['.pt-evaluate-team-panel','bo-teams','指定队伍','cards'],['.pt-evaluate-item-panel','bo-equipment','区域道具','equipment']]){
  const section=q(selector);section.dataset.section=id;section.classList.add('bo-section');section.querySelector('h3').innerHTML=designIcon(mark)+name;
  if(id==='bo-songs'){section.querySelector('.bo-section-heading').after(el('p','bo-note song-instruction','点击歌曲可替换，点击等级切换难度。'));}
 }
 // Team selection precedes the lower-frequency equipment settings in the accepted page.
 liveSettings.after(elements.ptEvaluateTeamPanel);elements.ptEvaluateTeamPanel.after(elements.ptEvaluateItemPanel);elements.ptEvaluateItemPanel.after(q('.activity-song-card'));
 function render(player,event,point){
  controls.render(player,event);
  eventSelection.render(player,event);
  context.textContent=(Number(player.currentEvent)?'已选择活动 · ':'自定义活动 · ')+(eventTypeNames[event.eventType]||event.eventType);
  const isCustom=Number(player.currentEvent)===0;edit.textContent=isCustom?'编辑加成':'以此创建自定义';
  const family=point?'活动 PT':'综合力';summary.replaceChildren(el('p','pb-help',`${isCustom?'自定义':'活动预设'} · 当前活动加成计入${family}。`));
  const overview=el('div','pb-overview'),left=el('div'),right=el('div','pb-match-summary');overview.append(left,right);summary.append(overview);
  const label=(text,count='')=>{const n=el('div','pb-summary-label',text);if(count)n.append(el('small','',count));return n;};
  left.append(label('属性加成'));const attrs=el('div','pb-attribute-summary');for(const a of event.attributes||[]){const item=el('span');item.append(assetImage(attributeIconUrls(a.attribute),'',attributeNames[a.attribute])||el('span'),el('span','',attributeNames[a.attribute]),el('b','',`+${a.percent}%`));attrs.append(item);}if(!attrs.children.length)attrs.textContent='未设置';left.append(attrs,label('角色加成',`${event.characters?.length||0} 位`));
  const chars=el('div','pb-character-summary');for(const c of event.characters||[]){const item=el('span');const name=gameText(getCore()?.characters[c.characterId]?.characterName,`角色 ${c.characterId}`);item.title=name;item.append(assetImage(characterIconUrls(c.characterId),'',name)||el('span'),el('b','',`+${c.percent}%`));chars.append(item);}left.append(chars);
  right.append(label('属性与角色同时匹配',family));const metrics=el('div','pb-metrics');const metric=(title,value)=>{const n=el('div');if(title)n.append(el('span','',title));n.append(el('strong','',`+${value||0}%`));return n;};metrics.append(metric('',event.eventAttributeAndCharacterBonus?.[point?'pointPercent':'parameterPercent']));right.append(metrics,el('p','pb-help',`同时满足已选属性和角色时，额外增加${family}加成。`));
  if(!point){const extra=el('div','pb-stat-parameters');for(const [key,title]of[['performance','演出'],['technique','技巧'],['visual','形象']])extra.append(metric(title,event.eventCharacterParameterBonus?.[key]));right.append(extra);}
  const cardrow=el('div','pb-card-summary-row');cardrow.append(label('指定卡牌',`${event.members?.length||0} 张`));const cards=el('div','pb-card-summary');for(const m of event.members||[]){const c=getCore()?.cards[m.situationId],item=el('span');item.title=`#${m.situationId} · ${gameText(c?.prefix,'')}`;const art=assetImage(cardIconUrls({cardId:m.situationId,card:c,illustTrainingStatus:true}),'pb-card-art',`卡牌 ${m.situationId}`);if(art)item.append(art);item.append(el('b','',`+${m.percent}%`));cards.append(item);}if(!cards.children.length)cards.append(el('span','pb-muted','未设置'));cardrow.append(cards);summary.append(cardrow);
 }
 function editBonuses(){openBonusEditor({getPlayer,getCore,getProfileId,eventSnapshot,writePlayer,renderForms});}

 return {render};
}
