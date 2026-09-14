import { mountActivityLayout } from './activity-layout.js';
import { icon } from './shell.js';
import { ptEvaluateLiveVariant, ptMaximizeLiveVariant } from '../models/player-settings.js?v=3';
import { starIconUrls, assetImage } from '../assets/index.js';

export function bonusApplication(eventType,mode,live) {
  if(mode==='maximize')return 'stat';
  if(mode==='scoreRange')return ['medley','versus'].includes(eventType)?'stat':'point';
  return ['medley','versus'].includes(eventType)||['challenge_cp','festival'].includes(live)?'stat':'point';
}
export function createActivityUI({elements,getPlayer,writePlayer,renderForms,eventSnapshot,activityModeForEvent,getCore,getProfileId}){
  const field=elements.eventCombinedPercent,match=field.closest('.activity-match-bonus');
  const changeCustomType=type=>{
    const player=getPlayer();if(Number(player.currentEvent)!==0)return;
    player.eventOverrides[0].eventType=type;player.activityMode=activityModeForEvent(player.eventOverrides[0]);writePlayer(player);renderForms(player);
  };
  const parameters=document.createElement('section');parameters.className='activity-parameters';
  parameters.innerHTML=`<h3 class="activity-subheading">${icon('M4 7h16M4 17h16M9 4v6m6 4v6')}演出参数</h3>`;
  elements.calculationMode.closest('.activity-grid').after(parameters);
  parameters.append(elements.scoreRangeControls,elements.ptMaximizeControls,elements.ptEvaluateControls);
  elements.calculationMode.querySelector('legend').insertAdjacentHTML('afterbegin',icon('M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18m0 5v4l3 2'));
  const eventLabel=elements.eventSearch.closest('label').querySelector('span');eventLabel.classList.add('activity-subheading');eventLabel.insertAdjacentHTML('afterbegin',icon('M4 6h16v14H4zM8 3v6m8-6v6M4 11h16'));
  const breaks=document.createElement('details');breaks.className='activity-limit-breaks';breaks.innerHTML='<summary>突破加成</summary><div class="mastery-matrix"></div>';match.after(breaks);
  const layout=mountActivityLayout({elements,getPlayer,getCore,getProfileId,writePlayer,renderForms,eventSnapshot,parameters,changeCustomType,breaks});
  function render(player){
    const event=eventSnapshot(player.currentEvent,player);if(!event)return;

    parameters.hidden=['maximize','ptEvaluate'].includes(player.calculationMode);
    const live=player.calculationMode==='ptEvaluate'?ptEvaluateLiveVariant(player.ptEvaluate,event.eventType):ptMaximizeLiveVariant(player.ptMaximize,event.eventType);
    const point=bonusApplication(event.eventType,player.calculationMode,live)==='point';
    field.dataset.bonusKind=point?'pointPercent':'parameterPercent';field.value=String(event.eventAttributeAndCharacterBonus?.[field.dataset.bonusKind]||0);
    const label=field.closest('label');if(label){const text=label.querySelector('span');if(text)text.textContent=point?'活动 PT':'综合力';}
    for(const input of [elements.eventCharacterParamPerformance,elements.eventCharacterParamTechnique,elements.eventCharacterParamVisual])input.closest('label').hidden=point;
    layout.render(player,event,point);
    const matrix=breaks.querySelector('.mastery-matrix');matrix.replaceChildren();breaks.hidden=!point&&!(event.limitBreaks?.length);
    const header=document.createElement('div');header.className='mastery-row';header.append(document.createElement('span'));for(let rank=0;rank<=4;rank++){const n=document.createElement('span');n.textContent=`突破 ${rank}`;header.append(n);}matrix.append(header);
    for(let rarity=5;rarity>=1;rarity--){const row=document.createElement('div');row.className='mastery-row';const image=assetImage(starIconUrls(rarity),'',`${rarity} 星`);row.append(image||document.createElement('span'));for(let rank=0;rank<=4;rank++){const input=document.createElement('input');input.type='number';input.min='0';input.step='any';input.value=String(Number(Number(event.limitBreaks?.find(v=>v.rarity===rarity&&v.rank===rank)?.percent||0).toFixed(4)));input.disabled=true;input.setAttribute('aria-label',`${rarity} 星突破 ${rank} 加成百分比`);input.onchange=()=>{if(!input.reportValidity())return;const latest=getPlayer(),target=latest.eventOverrides[0];target.limitBreaks=(target.limitBreaks||[]).filter(v=>v.rarity!==rarity||v.rank!==rank);target.limitBreaks.push({rarity,rank,percent:Number(input.value)});writePlayer(latest);};row.append(input);}matrix.append(row);}
  }
  return {render};
}
