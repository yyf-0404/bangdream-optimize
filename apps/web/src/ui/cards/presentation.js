import {assetOriginUrl,cardArtUrls} from '../../assets/index.js';
import {bandNames,attributeNames,serverNames} from './rules.js';
import {cardGrowth,cardCapabilities,cardGrowthText} from './growth.js';
import {cardSkillInfo} from './skill.js';
import {setCardImage,imageSources} from './images.js';
import {cardStatValues} from './stats.js';



const cpBase=new URL('./',import.meta.url);
const cpIcons=assetOriginUrl()+'/res/icon/';
const cpEscape=value=>String(value??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const cpSvg=(path)=>`<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${path}"/></svg>`;
const cpSwap=cpSvg('M4 7h15m-4-4 4 4-4 4M20 17H5m4-4-4 4 4 4');
const cpFlag=cpSvg('M5 21V3m0 1h13l-3 4 3 4H5');
export function cardPresentationData(card){return {...card,owned:card.owned??true};}
function cpShownCard(card){return cardPresentationData(card);}
export function cardBriefMarkup(card,{showName=false,captain=false,order=null}={}){
 const c=cpShownCard(card),g=cardGrowth(c),skill=cardSkillInfo(c),caps=cardCapabilities(c);
 const label=`${c.name}，${c.title}，卡牌 ${c.id}，${attributeNames[c.attribute]}，${c.rarity} 星，技能等级 ${c.skill}，突破 ${g.mastery}，${skill.category}，技能加成 ${skill.value}`;
 const skillIcons=skill.extra.map(type=>`<img class="cp-skill-icon" src="${new URL('skill-icons/'+type+'.png',cpBase).href}" alt="${type}">`).join('');
 return `<div class="cp-index">${order!=null?`<span class="cp-order" aria-label="技能顺序 ${order}">${order}</span>`:''}<span class="cp-id" title="卡牌 ID ${c.id}">#${c.id}</span>${captain?`<span class="cp-captain" title="队长" aria-label="队长">${cpFlag}<span>LEAD</span></span>`:!c.owned?`<span class="cp-missing" title="未持有" aria-label="未持有">${cpSvg('M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18')}</span>`:''}</div><div class="cp-media"><button type="button" class="cp-main" data-cp-open="${c.id}" aria-label="${cpEscape(label)}" title="${cpEscape(label+'\n'+skill.description+'\n'+cardGrowthText(c))}"><span class="cp-art art-wrap"><img class="cp-thumb card-art" alt="${cpEscape(c.name)}" loading="lazy" decoding="async"><span class="image-fallback" hidden>NO ART</span><img class="cp-attribute" src="${cpIcons+c.attribute}.svg" alt="${cpEscape(c.attribute)}"><img class="cp-rarity" src="${cpIcons}star_${c.rarity}.png" alt="R${c.rarity}">${c.owned&&g.mastery>0?`<span class="cp-mastery" data-rank="${g.mastery}" title="突破等级 ${g.mastery}" role="img" aria-label="突破等级 ${g.mastery}"><img src="${new URL('game-icons/limit-break.png',cpBase).href}" alt=""><b>${g.mastery}</b></span>`:''}</span></button><button class="cp-flip" type="button" data-cp-flip="${c.id}" aria-pressed="${g.illustTrained}" aria-label="卡牌 ${c.id}：当前特训${g.illustTrained?'后':'前'}图，切换到特训${g.illustTrained?'前':'后'}图" title="${caps.training.length<2?(g.illustTrained?'此卡只有特训后图':'此卡没有特训后图'):'切换前后图，不改变养成状态'}" ${caps.training.length<2?'disabled':''}>${cpSwap}</button></div><div class="cp-copy"><span class="cp-name" title="${cpEscape(c.name)}">${cpEscape(c.name)}</span><span class="cp-skill-row"><span class="cp-skill" data-long="${skill.notation.length>6}" role="img" aria-label="${cpEscape(skill.category+'，'+skill.value)}" title="${cpEscape(skill.description)}">${skill.notation?`<b>${cpEscape(skill.notation)}</b>`:''}${skillIcons}</span><span class="cp-skill-level" title="技能等级"><small>Lv.</small> <b>${c.owned?c.skill:'—'}</b></span></span></div>`;
}
function cpPaintArt(node,card){
 const c=cpShownCard(card),g=cardGrowth(c),image=node.querySelector('.cp-thumb');
 if(image&&image.dataset.cpVariant!==String(g.illustTrained)){image.dataset.cpVariant=String(g.illustTrained);setCardImage(image,c);}
 const flip=node.querySelector('.cp-flip');
 if(flip){flip.setAttribute('aria-pressed',String(g.illustTrained));flip.setAttribute('aria-label',`卡牌 ${c.id}：当前特训${g.illustTrained?'后':'前'}图，切换到特训${g.illustTrained?'前':'后'}图`);}
}
export function bindCardBrief(node,{resolve,onOpen,onIllustrationChange}={}){
 let local=resolve();
 cpPaintArt(node,local);
 if(node._cpBound)return;node._cpBound=true;
 node.addEventListener('click',event=>{
  const flip=event.target.closest('[data-cp-flip]');
  if(flip){event.stopPropagation();if(flip.disabled)return;const c=local;local={...c,growth:{...cardGrowth(c),illustTrained:!cardGrowth(c).illustTrained}};onIllustrationChange?.(resolve(),local.growth.illustTrained);cpPaintArt(node,local);return;}
  if(event.target.closest('.cp-copy')){event.stopPropagation();node.querySelector('.cp-main').click();return;}
  if(event.target.closest('[data-cp-open]')&&onOpen){event.stopPropagation();onOpen(resolve(),node);}
 });
}
export function resetCardIllustration(){}
function cpFullSources(c,trained){return cardArtUrls({card:c.record,illustTrainingStatus:trained}).concat(imageSources({...c,growth:{...cardGrowth(c),illustTrained:trained}}));}
export function createCardDetails({editable=false,onApply,loadCardDetail}={}){
 const dialog=document.createElement('dialog');dialog.className='cp-detail-dialog';dialog.setAttribute('aria-label','卡牌详情');
 dialog.dataset.editable=String(editable);document.body.append(dialog);
 let current,source,draft,opener,artSequence=0,detailSequence=0,detailState='idle';
 const query=selector=>dialog.querySelector(selector);
 function close(){artSequence++;detailSequence++;dialog.close();if(opener?.isConnected)opener.focus({preventScroll:true});}
 function art(){
  const sequence=++artSequence,chosen=draft.illustTrained,sources=cpFullSources(current,chosen),image=query('.cp-full-art');let index=0;
  image.hidden=false;query('.cp-art-status').textContent='';image.dataset.state='loading';
  const finish=()=>{if(sequence!==artSequence)return;image.dataset.state='ready';const actual=image.src.includes('after_training')||(!image.src.includes('_normal.')&&!!current.trained);query('.cp-art-status').textContent=actual!==chosen?'所选原画暂不可用，显示同卡可用图片':'';};
  image.onload=finish;image.onerror=()=>{if(sequence!==artSequence)return;if(++index<sources.length)image.src=sources[index];else{image.hidden=true;query('.cp-art-status').textContent='图片暂不可用';}};
  image.src=sources[0];
  query('.cp-art-stage').style.backgroundImage=`url("${imageSources({...current,growth:draft})[0]}")`;
  dialog.querySelectorAll('[data-cp-variant]').forEach(b=>b.setAttribute('aria-pressed',String((b.dataset.cpVariant==='true')===chosen)));
 }
 function skill(){
  const input=query('[name=skill]');
  if(input&&!input.validity.valid){query('.cp-detail-skill-value').textContent='—';query('.cp-detail-skill-copy').textContent='请输入 1–5 的整数技能等级。';return;}
  const s=cardSkillInfo({...current,skill:Number(input?.value)||current.skill});query('.cp-detail-skill-value').textContent=s.value;query('.cp-detail-skill-copy').textContent=s.description;
 }
 function stats(){
  const valid=[...dialog.querySelectorAll('input[name=level],input[name=mastery]')].every(input=>input.validity.valid);
  const values=valid?cardStatValues({...current,level:Number(query('[name=level]')?.value??current.level),growth:{...draft,
   trained:query('[name=trained]')?.checked??draft.trained,mastery:Number(query('[name=mastery]')?.value??draft.mastery),
   episodes:draft.episodes.map((value,index)=>query('[name=episode'+index+']')?.checked??value)}}):null;
  for(const node of dialog.querySelectorAll('[data-cp-stat]'))node.textContent=values?values[node.dataset.cpStat].toLocaleString('zh-CN'):'—';
  if(valid&&!values&&loadCardDetail&&detailState==='idle'){
   detailState='loading';
   const sequence=detailSequence,id=current.id;
   Promise.resolve().then(()=>loadCardDetail(id)).then(detail=>{
    if(sequence!==detailSequence)return;
    if(!detail?.stat)throw new Error('Missing card stats');
    current={...current,record:{...current.record,stat:detail.stat}};detailState='ready';stats();
   }).catch(()=>{if(sequence===detailSequence){detailState='failed';stats();}});
  }
  query('.cp-stats-note').textContent=values?'按卡牌等级、特训、剧情与突破计算':!valid?'请补全有效的养成数值':detailState==='loading'?'正在读取当前等级的能力资料…':detailState==='failed'?'能力资料读取失败，重新打开详情可重试':'当前等级的能力资料暂缺';
 }
 function open(card,{context=editable?'当前档案':'本次计算',trigger}={}){
  detailSequence++;detailState='idle';source=card;current=cardPresentationData(card);draft={...cardGrowth(cpShownCard(current))};opener=trigger||document.activeElement;
  const c=current,caps=cardCapabilities(c),g=draft;
  const stateRow=(name,title,checked,canEdit,note)=>`<div class="cp-state-row"><div><span>${title}</span>${note?`<small>${note}</small>`:''}</div>${editable?`<label class="cp-state-control"><input type="checkbox" name="${name}" ${checked?'checked':''} ${canEdit?'':'disabled'} aria-label="${title}"><span>${name==='trained'?(checked?'已特训':'未特训'):(note==='无此剧情'?'不适用':checked?'已读':'未读')}</span><i aria-hidden="true"></i></label>`:`<span class="cp-read-state" data-complete="${checked}">${name==='trained'?(checked?'已特训':'未特训'):(note==='无此剧情'?'不适用':checked?'已读':'未读')}</span>`}</div>`;
  const dates=Object.entries(serverNames).map(([key,label])=>`<div><img src="${cpIcons+key}.svg" alt=""><span>${label}</span><time>${c.releaseDates?.[key]?new Intl.DateTimeFormat('zh-CN',{year:'numeric',month:'2-digit',day:'2-digit',timeZone:'Asia/Shanghai'}).format(c.releaseDates[key]):'暂无记录'}</time></div>`).join('');
  const metric=(name,label,value,max)=>`<label class="cp-detail-metric"><span>${label}</span><div>${editable?`<input name="${name}" aria-label="${label}" type="number" required min="${name==='mastery'?0:1}" max="${max}" step="1" value="${value}">`:`<strong>${value}</strong>`}<small>/ ${max}</small></div></label>`;
  dialog.innerHTML=`<header class="cp-detail-header"><div><h2>卡牌详情</h2><span>${cpEscape(context)} <i>·</i> #${c.id}</span></div><button type="button" class="cp-detail-close" aria-label="关闭卡牌详情">${cpSvg('M6 6l12 12M18 6 6 18')}</button></header><div class="cp-detail-scroll"><div class="cp-detail-layout"><div class="cp-detail-media"><div class="cp-art-stage"><img class="cp-full-art" alt="${cpEscape(c.title+' · '+c.name)}"></div><div class="cp-art-toolbar"><span>原画展示</span><div class="cp-art-options" role="group" aria-label="切换特训前后图片">${[false,true].map(v=>`<button type="button" data-cp-variant="${v}" aria-pressed="${v===g.illustTrained}" ${caps.training.includes(v)?'':'disabled'}>特训${v?'后':'前'}</button>`).join('')}</div></div><p class="cp-art-status" role="status"></p><section class="cp-release"><h3>发布时间</h3><div class="cp-release-list">${dates}</div></section></div><div class="cp-detail-content"><div class="cp-detail-identity"><p class="cp-character">${cpEscape(c.name)}</p><h3>${cpEscape(c.title)}</h3><div class="cp-game-tags"><img class="cp-band" src="${cpIcons}band_${c.band}.svg" alt="${cpEscape(bandNames[c.band]||'乐队')}"><span><img src="${cpIcons+c.attribute}.svg" alt="">${attributeNames[c.attribute]}</span><span><img src="${cpIcons}star_${c.rarity}.png" alt="">${c.rarity} 星</span></div></div><section class="cp-detail-skill"><div><span>技能加成</span><strong class="cp-detail-skill-value"></strong></div><div class="cp-detail-skill-text"><p class="cp-detail-skill-copy"></p></div></section><section class="cp-detail-growth"><div class="cp-section-label"><h3>卡牌养成</h3>${editable?`<label class="cp-owned"><input name="owned" type="checkbox" ${c.owned?'checked':''}>已持有</label>`:''}</div><div class="cp-detail-metrics">${metric('level','卡牌等级',c.level,c.maxLevel||60)}${metric('skill','技能等级',c.skill,5)}${metric('mastery','突破等级',g.mastery,4)}</div><div class="cp-state-list">${stateRow('trained','特训',g.trained,caps.training.length>1,caps.training.length<2?(g.trained?'此卡固定特训':'此卡不可特训'):'')}${caps.episodes.map((flag,i)=>stateRow('episode'+i,'剧情 '+(i+1),g.episodes[i],flag===1,flag===0?'无此剧情':flag===2?'默认已读':'')).join('')}</div></section></div></div></div><footer class="cp-detail-footer"><p>${editable?'保存后更新当前档案':context==='本次计算'?'本次结果的养成记录':'当前档案的养成记录'}</p><div><button type="button" data-cp-cancel>${editable?'取消':'关闭'}</button>${editable?'<button type="button" class="cp-save" data-cp-save>保存修改</button>':''}</div></footer>`;
  query('.cp-detail-skill').insertAdjacentHTML('afterend',`<section class="cp-detail-stats" aria-label="卡牌综合力"><h3>卡牌综合力</h3><dl>${[['total','合计'],['performance','演出'],['technique','技巧'],['visual','形象']].map(([key,label])=>`<div><dt>${label}</dt><dd data-cp-stat="${key}">—</dd></div>`).join('')}</dl><p class="cp-stats-note"></p></section>`);
  query('.cp-detail-close').onclick=close;query('[data-cp-cancel]').onclick=close;
  dialog.querySelectorAll('[data-cp-variant]').forEach(b=>b.onclick=()=>{draft.illustTrained=b.dataset.cpVariant==='true';art();});
  dialog.querySelectorAll('.cp-state-control input').forEach(input=>input.onchange=()=>{input.nextElementSibling.textContent=input.name==='trained'?(input.checked?'已特训':'未特训'):(input.checked?'已读':'未读');stats();});
  dialog.querySelectorAll('input[name=level],input[name=mastery]').forEach(input=>input.addEventListener('input',stats));
  if(editable){query('[name=skill]').oninput=skill;query('[data-cp-save]').onclick=()=>{if(![...dialog.querySelectorAll('input[type=number]')].every(n=>n.reportValidity()))return;
    const next={level:Number(query('[name=level]').value),skill:Number(query('[name=skill]').value),owned:query('[name=owned]').checked,growth:cardGrowth({...c,growth:{...draft,trained:query('[name=trained]').checked,mastery:Number(query('[name=mastery]').value),episodes:[query('[name=episode0]').checked,query('[name=episode1]').checked]}})};
    resetCardIllustration(c);onApply?.(source,next);close();};}
  skill();stats();art();if(!dialog.open)dialog.showModal();query('.cp-detail-scroll').scrollTop=0;
 }
 dialog.addEventListener('cancel',e=>{e.preventDefault();close();});
 return {open,close,dialog};
}
