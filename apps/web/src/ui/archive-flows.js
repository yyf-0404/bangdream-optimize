import {designFragment} from './approved/templates.js';
import {importSourceMarkup,importDoneMarkup,exportMarkup,archiveIcon} from './approved/archive-flows.js';
import {serverIconUrls} from '../assets/index.js';

export const escapeHtml=value=>String(value??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
export const servers={cn:'国服',jp:'日服',en:'国际服',tw:'台服',kr:'韩服'};
const flag=code=>`<img src="${escapeHtml(serverIconUrls(code)[0])}" alt="${servers[code]}" class="server-flag" width="24" height="19">`;
const flagInline=code=>`<img src="${escapeHtml(serverIconUrls(code)[0])}" alt="${servers[code]}" class="server-flag server-flag--inline" width="17" height="13">`;
export const flowHelpers={escapeHtml,servers,flag,flagInline,number:n=>Number(n).toLocaleString('zh-CN'),icon:archiveIcon,serverChoices:(name,value)=>`<div class="server-choices">${Object.entries(servers).map(([code,label])=>`<label><input type="radio" name="${name}" value="${code}" ${code===value?'checked':''}><span>${flag(code)}${label}</span></label>`).join('')}</div>`};
export function createArchiveFlow(title,caption){
 const dialog=designFragment('flow-dialog');dialog.removeAttribute('id');dialog.setAttribute('aria-label',title);dialog.removeAttribute('aria-labelledby');
 dialog.querySelector('h2').textContent=title;dialog.querySelector('.eyebrow').textContent=caption;
 const body=dialog.querySelector('#flow-body');for(const n of dialog.querySelectorAll('[id]'))n.removeAttribute('id');
 const close=dialog.querySelector('.icon-button');close.type='button';close.setAttribute('aria-label','关闭窗口');close.innerHTML=archiveIcon('close');close.onclick=()=>dialog.close();
 (document.querySelector('#archive-dialog-host')||document.querySelector('#archive-design')).append(dialog);
 const trigger=document.activeElement;dialog.addEventListener('close',()=>{dialog.remove();trigger?.focus();},{once:true});
 return {dialog,body};
}

export function openImportSource({profile,onRead}){
 const {dialog,body}=createArchiveFlow('导入配置','先核对，再应用');
 const d={source:'account',server:profile.server,playerId:profile.playerId||'',format:'base64',text:''};let busy=false;
 function paint(){
  body.innerHTML=importSourceMarkup({...flowHelpers,d,p:profile});
  const form=body.querySelector('form');form.noValidate=false;
  const sync=()=>{d.playerId=body.querySelector('#import-id')?.value??d.playerId;d.text=body.querySelector('#import-text')?.value??d.text;};
  body.querySelectorAll('[data-source]').forEach(b=>b.onclick=()=>{if(busy)return;sync();d.source=b.dataset.source;paint();});
  body.querySelectorAll('[name=import-server]').forEach(input=>input.onchange=()=>{sync();d.server=input.value;paint();});
  const format=body.querySelector('#import-format');if(format)format.onchange=()=>{sync();d.format=format.value;paint();};
  body.querySelectorAll('[data-close-flow]').forEach(b=>b.onclick=()=>dialog.close());
  form.onsubmit=async e=>{e.preventDefault();if(busy)return;sync();const error=body.querySelector('#import-error');error.textContent='';const input=body.querySelector(d.source==='account'?'#import-id':'#import-text');if(d.source==='account'?!/^[1-9]\d*$/.test(String(d.playerId)):!d.text.trim()){error.textContent=d.source==='account'?'请输入有效的玩家 ID':'请粘贴配置内容';input.focus();return;}
   busy=true;const button=form.querySelector('[type=submit]');button.disabled=true;button.textContent='正在读取…';
   try{const result=await onRead({...d});if(result){dialog.querySelector('h2').textContent='导入完成';body.innerHTML=importDoneMarkup({...flowHelpers,p:result});body.querySelectorAll('[data-close-flow],a').forEach(b=>b.addEventListener('click',()=>dialog.close()));}}catch(e){error.textContent=e.message||String(e);input.focus();}finally{busy=false;if(dialog.open){button.disabled=false;button.textContent='读取并预览';}}
  };
 }
 paint();dialog.showModal();
}

export function openExportFlow({profile,getPayload,copy,save}){
 const {dialog,body}=createArchiveFlow('导出配置','带走所需配置');let format='base64',payload='',token=0;
 async function paint(){const current=++token;payload='';body.innerHTML=exportMarkup({...flowHelpers,p:profile,compact:format==='base64',payload});
  const field=body.querySelector('textarea'),status=body.querySelector('#export-status'),copyButton=body.querySelector('#copy-export'),download=body.querySelector('#download-export');copyButton.disabled=download.disabled=true;
  body.querySelectorAll('[data-close-flow]').forEach(b=>b.onclick=()=>dialog.close());body.querySelectorAll('[name=export-format]').forEach(r=>r.onchange=()=>{format=r.value;paint();});
  copyButton.onclick=async()=>{try{await copy(payload);status.textContent='配置已复制';}catch(e){status.textContent=e.message||String(e);}};
  download.onclick=async()=>{
   download.disabled=true;status.textContent='正在保存…';
   try{
    const result=await save({fileName:`${profile.name}-${format}.json`,text:payload});
    if(current===token&&dialog.open)status.textContent=result==='cancelled'?'已取消保存，配置仍可复制或重新保存':result==='saved'?'配置文件已保存':'已生成下载文件';
   }catch(e){if(current===token&&dialog.open)status.textContent=e.message||String(e);}
   finally{if(current===token&&dialog.open)download.disabled=false;}
  };
  try{const result=await getPayload(format);if(current!==token||!dialog.open)return;payload=result;field.value=result;copyButton.disabled=download.disabled=false;}catch(e){if(current===token)status.textContent=e.message||String(e);}
 }
 dialog.showModal();paint();
}
