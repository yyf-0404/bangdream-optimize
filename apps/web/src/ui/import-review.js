import {importReviewMarkup} from './approved/archive-flows.js?v=3';
import {createArchiveFlow,flowHelpers} from './archive-flows.js?v=3';
export function importChanges(before,after){
  const result={};
  for(const field of ['cardList','customCards','areaItem','characterBouns']){
    const old=before[field]||{},next=after[field]||{};
    result[field]={added:Object.keys(next).filter(id=>!(id in old)).length,removed:Object.keys(old).filter(id=>!(id in next)).length,changed:Object.keys(next).filter(id=>id in old&&JSON.stringify(old[id])!==JSON.stringify(next[id])).length};
  }
  return result;
}
export function reviewImport(before,after,options={}){
 const changes=importChanges(before,after),{dialog,body}=createArchiveFlow('核对导入变化','导入配置');dialog.classList.add('wide');
 const d={source:options.source||'paste',format:options.format||'base64',server:after.server,destination:'current',newName:(options.name||'档案')+' 导入'};
 const p={name:options.name||'所选档案',server:before.server};
 const r={customCount:Object.keys(after.customCards||{}).length,cards:Object.keys(after.cardList||{}).length,items:Object.keys(after.areaItem||{}).length,characters:Object.keys(after.characterBouns||{}).length,rows:[['cardList','游戏卡牌'],['customCards','自定义卡牌'],['areaItem','区域道具'],['characterBouns','角色加成']].map(([key,label])=>{const c=changes[key];return [label,`${Object.keys(before[key]||{}).length} → ${Object.keys(after[key]||{}).length}`,`新增 ${c.added} · 修改 ${c.changed} · 移除 ${c.removed}`];})};
 let result=false;
 function paint(){body.innerHTML=importReviewMarkup({...flowHelpers,d,p,r,isNew:d.destination==='new',importScope:()=>options.identity?`国服账号 · ${options.identity.channel==='ios'?'iOS':'bili安卓'}`:d.source==='account'?'主乐队公开资料':d.format==='base64'?'Base64 配置':'Bestdori Profile'});
  if(options.identity){
   const summary=body.querySelector('.import-summary'),text=document.createElement('small');
   text.className='cn-account-identity';text.textContent=`${options.identity.name||'游戏账号'} · ID ${options.identity.gameUid} · Lv. ${options.identity.rank}`;summary.append(text);
  }
  if(!options.allowNew)body.querySelector('input[value=new]').closest('label').remove();
  body.querySelectorAll('[name=destination]').forEach(radio=>radio.onchange=()=>{d.newName=body.querySelector('#import-new-name')?.value??d.newName;d.destination=radio.value;paint();});
  body.querySelector('#import-back').onclick=()=>dialog.close();
  body.querySelector('#apply-import').onclick=()=>{if(d.destination==='new'){d.newName=body.querySelector('#import-new-name').value.trim();if(!d.newName){body.querySelector('#new-import-error').textContent='请填写新档案名称';body.querySelector('#import-new-name').focus();return;}}result={destination:d.destination,name:d.newName};dialog.close();};
 }
 paint();return new Promise(resolve=>{
  if(options.signal?.aborted){dialog.remove();resolve(false);return;}
  const cancel=()=>{result=false;dialog.close();};options.signal?.addEventListener('abort',cancel,{once:true});
  dialog.addEventListener('close',()=>{options.signal?.removeEventListener('abort',cancel);resolve(result);},{once:true});dialog.showModal();
 });
}
