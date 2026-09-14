import {designIcon} from './fidelity.js';
import {showResultDiagnostics} from './result-dialogs.js';
import {designFragment} from './approved/templates.js';
import {decorateResult} from './approved/result-decoration.js';
let currentDiagnostic;
const el=(tag,cls,text)=>{const n=document.createElement(tag);n.className=cls||'';if(text!==undefined)n.textContent=text;return n;};
export function mountResultLayout(){
 const page=document.querySelector('#result-design'),panel=page.querySelector('.result-panel'),hero=page.querySelector('.page-hero');
 const actions=panel.querySelector('.panel-actions'),menu=el('details','result-more');menu.innerHTML='<summary aria-label="更多结果操作">···</summary>';const popup=el('div');popup.append(actions);menu.append(popup);
 const historyPanel=page.querySelector('.result-cache-panel'),dialog=el('dialog','result-history-drawer');const header=el('header','drawer-head');header.append(el('h2','','历史结果'));const close=el('button','icon-button','×');close.type='button';close.setAttribute('aria-label','关闭历史结果');close.onclick=()=>dialog.close();header.append(close);dialog.append(header,historyPanel);historyPanel.querySelector('h2').textContent='当前档案';page.append(dialog);
 const rerun=el('button','','重新计算');rerun.type='button';rerun.dataset.calculationSubmit='';rerun.onclick=()=>{menu.open=false;document.querySelector('.side-calculate').click();};const diagnostic=el('button','','计算诊断');diagnostic.type='button';diagnostic.onclick=()=>{menu.open=false;showResultDiagnostics(currentDiagnostic);};actions.prepend(rerun,diagnostic);actions.addEventListener('click',()=>{menu.open=false;});
 const history=el('button','text-button');history.type='button';history.innerHTML=designIcon('result')+'历史结果 <small id="result-history-count">0</small>';history.onclick=()=>dialog.showModal();const tools=el('div','heading-actions');tools.append(history,menu);hero.append(tools);
 historyPanel.querySelector('#result-cache-list').addEventListener('click',e=>{if(e.target.closest('[data-result-cache-action=restore]'))dialog.close();});
 historyPanel.querySelector('#clear-result-cache').textContent='清空历史';
 panel.querySelector('.panel-header').remove();
 const approved=designFragment('result-content'),status=approved.querySelector('#status-strip'),body=approved.querySelector('#result-body');
 status.innerHTML='<span id="result-run-state"></span>';status.append(document.querySelector('#metrics'));
 body.append(...panel.children);panel.replaceChildren(...approved.children);panel.className='result-panel';
}
export function applyResultLayout(root,result,diagnostic){
 currentDiagnostic=diagnostic;
 if(!root.querySelectorAll)return;
 root.classList.add('accepted-result-body');
 const page=root.closest('#result-design');if(!page)return;
 const state=page.querySelector('#result-run-state');state.replaceChildren();state.innerHTML=designIcon('result');state.append(document.createTextNode(diagnostic?.error?'计算失败':result?'计算完成':'尚未计算'));state.dataset.phase=diagnostic?.error?'failed':result?'complete':'idle';
 const overview=root.querySelector('.result-overview');if(overview&&overview.children.length>4){const secondary=el('div','secondary-metrics');for(const n of [...overview.children].slice(4)){n.className='';secondary.append(n);}overview.after(secondary);}
 const context=[];
 for(const diagnostic of root.querySelectorAll(':scope>.result-diagnostic')){
  const title=diagnostic.querySelector('h3')?.textContent;if(['计算场景','计分方式'].includes(title)){for(const n of diagnostic.querySelectorAll('dd'))context.push(n.textContent);diagnostic.remove();}
 }
 if(context.length)page.querySelector('.hero-copy>span').textContent=context.join(' · ');
 for(const section of root.querySelectorAll(':scope>.result-section')){
  const h=section.querySelector(':scope>h3');if(!h)continue;
  const header=el('div','result-section-title'),title=el('h2','',h.textContent);title.insertAdjacentHTML('afterbegin',designIcon('song'));title.querySelector('svg').classList.add('record-mark');header.append(title);h.replaceWith(header);
 }
 decorateResult(document);
 const empty=root.querySelector('.result-empty');if(!result&&empty){const statePanel=el('section','state-panel');statePanel.innerHTML='<span class="state-symbol">'+designIcon('result')+'</span><h2>还没有计算结果</h2><p>准备好活动、卡牌和道具配置后，就可以开始计算。</p>';const actions=el('div','state-actions'),go=el('button','primary','前往活动配置');go.type='button';go.onclick=()=>document.querySelector('[data-page=activity]').click();const history=el('button','text-button','查看历史结果');history.type='button';history.onclick=()=>page.querySelector('.result-history-drawer').showModal();actions.append(go,history);statePanel.append(actions);empty.replaceWith(statePanel);}
}
