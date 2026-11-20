import {designFragment} from '../../design/approved/templates.js';
import {mountDesignControls,hydrateDesignIcons} from '../../design/controls.js';
import {openBonusEditor} from './bonus-editor.js';
import { designIcon } from '../../design/fidelity.js';
import { assetImage, attributeIconUrls, characterIconUrls, cardIconUrls } from '../../shared/assets.js';
import { gameText } from '../../shared/preferences.js';
import { attributeNames } from '../../components/cards/rules.js';
import { cardModel } from '../../components/cards/model.js';
import { imageSources } from '../../components/cards/images.js';
import { mountEventSelection, eventTypeNames } from './event-selection.js';
import {eventCardPoolInfo, supportsEventCardPool} from '../../domain/event-card-pool.js';
import { timingOptimizationEnabled } from '../../domain/hit-timing.js';
const el=(tag,cls,text)=>{const n=document.createElement(tag);n.className=cls||'';if(text!==undefined)n.textContent=text;return n;};

function activitySwitch(id, title, noteId) {
 const label = el('label', 'ui-button activity-option');
 const input = el('input', 'ui-visually-hidden');
 input.id = id;
 input.type = 'checkbox';
 input.setAttribute('role', 'switch');
 input.setAttribute('aria-labelledby', `${id}-label`);
 input.setAttribute('aria-describedby', noteId);
 label.htmlFor = id;
 const name = el('span', 'ui-text', title);
 name.id = `${id}-label`;
 const mark = el('span', 'activity-option-mark');
 mark.setAttribute('aria-hidden', 'true');
 label.append(input, mark, name);
 return { label, input };
}

export function mountActivityLayout({elements,getPlayer,getCore,getProfileId,writePlayer,renderForms,eventSnapshot,parameters,changeCustomType,breaks}){
 const root=document.querySelector('#aurora-soft-study'),q=s=>root.querySelector(s),target=q('.activity-selector-card');
 const approved=designFragment('activity-target');hydrateDesignIcons(approved);
 const legacy=q('.activity-selector-content');legacy.hidden=true;root.append(legacy);
 const fieldset=elements.calculationMode,original=[...fieldset.querySelectorAll('input')];
 const replacement=approved.querySelector('fieldset');
 for(const input of replacement.querySelectorAll('input'))input.replaceWith(original.find(n=>n.value===input.value));
 fieldset.className='bo-target-group';fieldset.replaceChildren(...replacement.childNodes);replacement.replaceWith(fieldset);
 target.className='activity-selector-card bo-section';target.replaceChildren(...approved.childNodes);
 const cardScopeNote = el('span', 'ui-visually-hidden');
 cardScopeNote.id = 'event-card-scope-note';
 const cardScope = activitySwitch('event-available-cards-only', '仅限本服活动可用卡牌', cardScopeNote.id);
 cardScope.label.append(cardScopeNote);
 const timingExplanation = el('span', 'ui-text activity-timing-explanation', '在 Perfect 判定范围内调整音符的击打时机，寻找得分更高的方案。结果页会显示建议提前或延后的音符及对应帧数。');
 timingExplanation.id = 'hit-timing-note';
 timingExplanation.popover = 'auto';
 const timing = activitySwitch('hit-timing-optimization', '击打优化', timingExplanation.id);
 const timingHelp = el('button', 'ui-button ui-button--icon activity-timing-help');
 timingHelp.type = 'button';
 timingHelp.title = '击打优化说明';
 timingHelp.setAttribute('aria-label', '查看击打优化说明');
 timingHelp.setAttribute('popovertarget', timingExplanation.id);
 timingHelp.innerHTML = designIcon('help');
 const timingOption = el('span', 'activity-timing-option');
 timingOption.append(timing.label, timingHelp, timingExplanation);
 const goalHeading = fieldset.querySelector('legend');
 const goalTitle = goalHeading.querySelector('.bo-subheading');
 goalTitle.id = 'calculation-mode-title';
 fieldset.setAttribute('aria-labelledby', goalTitle.id);
 goalHeading.classList.add('activity-option-heading');
 goalHeading.append(timingOption);
 const cardScopeNotice = el('span', 'ui-text activity-option-notice', '缺少本服活动结束时间，使用完整卡池。');
 cardScopeNotice.hidden = true;
 cardScope.input.addEventListener('change', event => {
  event.stopPropagation();
  const player = getPlayer();
  player.eventAvailableCardsOnly = cardScope.input.checked;
  writePlayer(player);
  renderForms(player);
 });
 timing.input.addEventListener('change', event => {
  event.stopPropagation();
  const player = getPlayer();
  if (timing.input.checked) player.hitTiming = true;
  else delete player.hitTiming;
  writePlayer(player);
  renderForms(player);
 });
 const context=target.querySelector('.bo-section-heading>span');
 const eventSelection=mountEventSelection({root,target,elements,getPlayer,getProfileId,eventSnapshot,changeCustomType});
 const eventTitle = target.querySelector('.bo-event-row .bo-subheading');
 const eventHeading = el('div', 'activity-option-heading');
 eventTitle.before(eventHeading);
 eventHeading.append(eventTitle, cardScope.label, cardScopeNotice);
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
  timingOption.hidden = timing.input.disabled = player.calculationMode !== 'maximize';
  if (timingOption.hidden && timingExplanation.matches(':popover-open')) timingExplanation.hidePopover();
  timing.input.checked = timingOptimizationEnabled(player);
  cardScope.label.hidden = cardScope.input.disabled = !supportsEventCardPool(player.calculationMode);
  cardScope.input.checked = player.eventAvailableCardsOnly === true;
  const scope=eventCardPoolInfo(player,event,{...getCore()?.cardsFix,...getCore()?.cards});
  cardScopeNote.textContent = scope.endAt === null
   ? '当前活动缺少本服结束时间，将使用全部已持有卡牌及已启用的自定义卡。'
   : `限本服活动结束前发布的卡牌，开启时排除自定义卡。${cardScope.input.checked ? `符合条件 ${scope.cardIds.length} 张。` : ''}`;
  cardScope.label.title = cardScopeNote.textContent;
  cardScopeNotice.hidden = cardScope.input.disabled || !cardScope.input.checked || scope.endAt !== null;
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
  const cardrow=el('div','pb-card-summary-row');cardrow.append(label('指定卡牌',`${event.members?.length||0} 张`));
  const cards=el('div','pb-card-summary');
  for(const m of event.members||[]){
   const card=cardModel(getCore(),player,m.situationId,undefined,getProfileId());
   const id=card.displayId||`#${m.situationId}`;
   const item=el('span');item.title=`${id} · ${card.title}`;
   const art=assetImage(card.custom?imageSources(card):cardIconUrls({cardId:card.id,card:card.record,illustTrainingStatus:true}),'pb-card-art',card.title);
   if(art)item.append(art);
   if(card.custom)item.append(el('small','pb-card-id',id));
   item.append(el('b','',`+${m.percent}%`));cards.append(item);
  }
  if(!cards.children.length)cards.append(el('span','pb-muted','未设置'));
  cardrow.append(cards);summary.append(cardrow);
 }
 function editBonuses(){openBonusEditor({getPlayer,getCore,getProfileId,eventSnapshot,writePlayer,renderForms});}

 return {render};
}
