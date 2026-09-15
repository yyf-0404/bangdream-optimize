import {revealValidationError} from './validation.js';
import {designFragment} from './approved/templates.js';
import {liveMarkup} from './approved/live-template.js';
import {calculationMarkup} from './approved/calculation-template.js';
import {allowedLiveVariants, ptEvaluateLiveVariant, ptEvaluateSupportsAuto, ptMaximizeLiveVariant} from '../models/player-settings.js';
import {designIcon} from './fidelity.js';

const names={solo:'自由演出',cooperative:'协力演出',challenge_cp:'挑战演出',versus:'竞演演出',festival:'团队演出',medley:'巡回演出'};
export function hydrateDesignIcons(root){
 const icons={'sliders-horizontal':'activity','audio-lines':'activity','circle-plus':'bonus','disc-3':'song',package:'equipment',search:'search','arrow-down':'arrow','layers-2':'layers','circle-help':'help'};
 for(const node of root.querySelectorAll('[data-lucide],[data-icon]'))node.innerHTML=designIcon(icons[node.dataset.lucide||node.dataset.icon]||node.dataset.icon);
}

// The hidden controls remain the solver's input adapter. Visible controls use
// the approved templates exclusively; changes are saved through the player store.
export function mountDesignControls({root,parameters,legacy,getPlayer,writePlayer,renderForms}){
 const bank=document.createElement('div');bank.hidden=true;bank.className='runtime-control-bank';
 bank.append(...legacy);root.append(bank);
 const live=designFragment('activity-live');live.className='bo-section pc-section';hydrateDesignIcons(live);
 root.querySelector('#bo-bonuses').after(live);
 document.querySelector('#controls').addEventListener('submit',e=>{
  const invalid=[parameters,live].filter(section=>!section.hidden).flatMap(section=>[...section.querySelectorAll('input,select')]).find(input=>!input.disabled&&!input.checkValidity());
  if(!invalid)return;
  e.preventDefault();e.stopImmediatePropagation();
  revealValidationError(new Error(invalid.validationMessage), {field:invalid, activatePage:page=>document.querySelector(`[data-page=${page}]`)?.click()});
 },true);
 let signature='',calcSignature='',calcShape='';
 const save=(path,value)=>{const player=structuredClone(getPlayer()),keys=path.split('.'),last=keys.pop();let target=player;for(const key of keys)target=target[key]??=(/^[0-9]+$/.test(key)?[]:{});target[last]=value;writePlayer(player);renderForms(player);};
 const changeSetting=e=>{
  const input=e.target.closest('[data-setting]');if(!input)return;
  if(!input.checkValidity())return;
  const old=input.dataset.setting.split('.').reduce((v,k)=>v?.[k],getPlayer());
  const value=typeof old==='boolean'?input.value==='true':typeof old==='number'||input.type==='number'?Number(input.value):input.value;
  if(old!==value)save(input.dataset.setting,value);
 };
 parameters.addEventListener('input',e=>{if(e.target.type==='number')changeSetting(e);});
 parameters.addEventListener('change',changeSetting);
 const changeLive=e=>{
  const input=e.target;if(!input.matches('input'))return;if(!input.reportValidity())return;
  const eventType=live.dataset.eventType;
  const paths={'pc-live':`liveVariantByEventType.${eventType}`,'pc-score':'scoreMode','pc-multiplier':'autoBaseMultiplier','pc-rank':'versusTeamRank'};
  const path=input.id==='pc-support'?'missionSupportPtBonus':paths[input.name];if(!path)return;
  const value=['pc-live','pc-score'].includes(input.name)?input.value:Number(input.value);
  if(path.split('.').reduce((v,k)=>v?.[k],getPlayer().ptEvaluate)!==value)save('ptEvaluate.'+path,value);
 };
 live.addEventListener('input',e=>{if(e.target.type==='number')changeLive(e);});
 live.addEventListener('change',changeLive);
 function render(player,event){
  live.hidden=player.calculationMode!=='ptEvaluate';live.dataset.eventType=event.eventType;
  const config=player.ptEvaluate,variant=ptEvaluateLiveVariant(config,event.eventType),auto=ptEvaluateSupportsAuto(variant);
  const a={liveVariant:variant,scoreMode:auto?config.scoreMode:'manual',autoSupported:auto};
  const next=JSON.stringify([config,event.eventType,player.server]);
  if(signature!==next){signature=next;const view=liveMarkup({a,allowed:allowedLiveVariants(event.eventType,'ptEvaluate'),config,profile:player,eventType:()=>event.eventType,R:{liveNames:names}});if(document.activeElement?.id!=='pc-support')live.querySelector('#pc-live-fields').innerHTML=view.fields;live.querySelector('#pc-score-note').textContent=view.note;}
  live.querySelector('#pc-live-summary').textContent=variant==='medley'?'三曲共用演出设置':'本次指定队伍';
  const m=player.calculationMode,t=event.eventType;
  const nextCalc=JSON.stringify([m,t,player.server,player.ptMaximize,player.scoreRange]);
  const p=player.ptMaximize,shape=JSON.stringify([m,t,player.server,p.liveVariantByEventType,p.teammateMode,p.cooperativeLeaderMode,p.festivalTeammateMode]);
  const editing=parameters.contains(document.activeElement)&&document.activeElement.type==='number';
  if(calcSignature!==nextCalc){calcSignature=nextCalc;parameters.hidden=!['ptMaximize','scoreRange'].includes(m);if(!editing||calcShape!==shape)parameters.innerHTML=parameters.hidden?'':calculationMarkup({state:player,server:player.server,m,t,l:ptMaximizeLiveVariant(p,t),R:{variants:{[t]:allowedLiveVariants(t,'ptMaximize')},names}});calcShape=shape;}
  const delta=parameters.querySelector('.calc-delta');if(delta){const n=player.scoreRange.targetTotalPt-player.scoreRange.currentPt;delta.textContent=n>0?'还需 '+n.toLocaleString('zh-CN')+' PT':'';}
 }
 return {live,render};
}
