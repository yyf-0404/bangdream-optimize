// Copied presentation functions; no fixture values or preview handlers.
export function decorateResult(document) {
 function displayEquipment(){
  const section=document.querySelector('.item-selection');
  if(!section)return;
  section.querySelectorAll('button.selected-item').forEach(button=>{
   const item=document.createElement('div');
   item.className=button.className;
   item.append(...button.childNodes);
   button.replaceWith(item);
  });
  section.querySelectorAll(':scope > .text-button').forEach(button=>button.remove());
  const title=section.querySelector(':scope > span');
  if(title){title.id='result-equipment-heading';section.setAttribute('role','group');section.setAttribute('aria-labelledby',title.id);}
 }
 const marks={
  music:'<path d="M9 18V5l11-2v13M9 8l11-2"/><ellipse cx="6" cy="18" rx="3" ry="3"/><ellipse cx="17" cy="16" rx="3" ry="3"/>',
  mixer:'<path d="M5 3v5m0 4v9M12 3v10m0 4v4M19 3v3m0 4v11M2 8h6v4H2zM9 13h6v4H9zM16 6h6v4h-6z"/>',
  team:'<circle cx="9" cy="8" r="3"/><path d="M3 21v-3a6 6 0 0 1 12 0v3M16 5a3 3 0 0 1 0 6m2 4a5 5 0 0 1 3 5"/>',
  plan:'<rect x="3" y="5" width="18" height="16" rx="2"/><path d="M7 3v4m10-4v4M3 11h18m-13 5 3 3 5-5"/>',
  result:'<path d="M4 10v4M8 6v12M12 3v18M16 6v12M20 10v4"/>'
 };
 function mark(node,name){
  if(!node||node.querySelector('.record-mark'))return;
  node.insertAdjacentHTML('afterbegin',`<svg class="record-mark" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${marks[name]}</svg>`);
 }
 function decorate(){
  displayEquipment();
  mark(document.getElementById('result-equipment-heading'),'mixer');
  mark(document.querySelector('.metric.main>span'),'result');
  document.querySelectorAll('.result-section-title h2').forEach(h=>mark(h,h.textContent==='演奏安排'?'plan':h.textContent==='使用队伍'?'team':'music'));
  document.querySelectorAll('.selected-item>img').forEach(img=>{
   const stage=document.createElement('span');stage.className='record-asset';
   const name=new URL(img.src).pathname.split('/').pop();
   const tones={'band_5.svg':'130,101,186','powerful.svg':'209,73,99','cool.svg':'74,114,199','happy.svg':'193,148,45','pure.svg':'55,151,126'};
   stage.style.setProperty('--asset-rgb',tones[name]||'161,123,145');
   img.replaceWith(stage);stage.append(img);
  });
  document.querySelectorAll('.song-top>.song-cover,.play-song>img').forEach(img=>{
   const sleeve=document.createElement('span');sleeve.className='record-sleeve';
   img.replaceWith(sleeve);sleeve.append(img);
  });
  document.querySelectorAll('.song-title').forEach((title,i)=>{
   if(title.querySelector('.record-order'))return;
   const small=title.querySelector('small');
   if(!small)return;
   const split=small.textContent.match(/^(第\s*\d+\s*曲\s*·\s*)(.*)$/);
   if(!split)return;
   const original=document.createElement('span');original.className='record-old-order';original.textContent=split[1];
   small.replaceChildren(original,document.createTextNode(split[2]));
   const order=document.createElement('span');order.className='record-order';order.textContent=String(i+1).padStart(2,'0');order.setAttribute('aria-label',`第 ${i+1} 曲`);
   title.prepend(order);
  });
  document.querySelectorAll('.song-top').forEach(top=>{
   if(top.querySelector('.composition-song-info'))return;
   const sleeve=top.querySelector('.record-sleeve'),identity=top.querySelector('.song-identity');
   if(!sleeve||!identity)return;
   const info=document.createElement('div');info.className='composition-song-info';
   top.prepend(info);info.append(sleeve,identity);
  });
 }

 decorate();
}
