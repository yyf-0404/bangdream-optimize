import { cardGrowth } from './growth.js';
import {bulkMarkup} from '../approved/bulk-template.js';

// Only enabled fields are edited. Already owned cards are excluded from add operations.
export function planBulk(cards, action, values = {}, fields = []) {
  return cards.filter(c=>action==='add'?!c.owned:c.owned).map(card=>{
    if(action==='remove')return {card,next:{...card,owned:false}};
    const next={...card,growth:structuredClone(cardGrowth(card)),owned:true};
    for(const key of fields){
      if(key==='level')next.level=values.level==='max'?card.maxLevel:Math.min(card.maxLevel,Math.max(1,Number(values.level)));
      else if(key==='skill')next.skill=Number(values.skill);
      else if(key==='episodes')next.growth.episodes=next.growth.episodes.map(()=>!!values.episodes);
      else next.growth[key]=values[key];
    }
    next.growth=cardGrowth(next);
    return {card,next};
  });
}
export async function reviewBulk(cards,action){
  const owned=cards.filter(c=>c.owned).length,missing=cards.length-owned,count=action==='add'?missing:owned;
  if(!count)return [];
  const dialog=document.createElement('dialog');dialog.className='bulk-dialog';dialog.setAttribute('aria-labelledby','bulk-title');
  dialog.innerHTML=bulkMarkup({action:action==='edit'?'growth':action,reviewIds:cards.map(c=>c.id),owned,missing,eligible:count});
  (document.querySelector('#card-library-unified')||document.body).append(dialog);
  const form=dialog.querySelector('form'),get=name=>form.elements.namedItem(name),apply=dialog.querySelector('.bulk-apply');
  function fields(){return action==='add'?['level','skill','mastery','trained','illustTrained','episodes']:[...form.querySelectorAll('[name=field]:checked')].map(n=>n.value);}
  function update(){
    for(const row of form.querySelectorAll('.bulk-template-row')){const check=row.querySelector('[name=field]'),enabled=!check||check.checked;row.classList.toggle('is-inactive',!enabled);for(const control of row.querySelectorAll('input:not([name=field]),select'))control.disabled=!enabled;}
    if(get('level')){get('level').hidden=get('levelMode').value!=='fixed';get('level').disabled=get('levelMode').disabled||get('level').hidden;get('level').required=true;}
    apply.disabled=action==='edit'&&!fields().length;
    form.querySelector('.bulk-review').textContent=apply.disabled?'请至少勾选一个要修改的项目。':'确认后更新当前档案；取消不会保存。';
  }
  form.addEventListener('input',update);form.addEventListener('change',update);update();
  dialog.querySelectorAll('[data-close]').forEach(b=>b.onclick=()=>dialog.close());
  let changes=[];
  form.onsubmit=e=>{e.preventDefault();update();if(apply.disabled||!form.reportValidity())return;const values={};for(const key of fields()){values[key]=key==='level'?(get('levelMode').value==='max'?'max':Number(get(key).value)):['trained','illustTrained','episodes'].includes(key)?get(key).value==='true':Number(get(key).value);}changes=planBulk(cards,action,values,fields());dialog.close('apply');};
  return new Promise(resolve=>{const trigger=document.activeElement;dialog.addEventListener('close',()=>{dialog.remove();trigger?.focus();resolve(changes);},{once:true});dialog.showModal();});
}
