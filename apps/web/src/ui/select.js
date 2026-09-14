import {searchOptions,moveOption,placeSelectPopup} from './select-model.js';

const svg=path=>`<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${path}</svg>`;
const chevron=svg('<path d="m7 10 5 5 5-5"/>'),check=svg('<path d="m5 12 4 4L19 6"/>'),searchIcon=svg('<circle cx="10.5" cy="10.5" r="6.5"/><path d="m16 16 4 4"/>');
const visible=node=>node.isConnected&&!!node.getClientRects().length;
const setAttribute=(node,name,value)=>{const text=String(value);if(node.getAttribute(name)!==text)node.setAttribute(name,text);};
const setText=(node,value)=>{if(node.textContent!==value)node.textContent=value;};

function selectLabel(select){
 if(select.getAttribute('aria-label'))return select.getAttribute('aria-label');
 if(select.getAttribute('aria-labelledby'))return select.getAttribute('aria-labelledby').split(/\s+/).map(id=>document.getElementById(id)?.textContent||'').join(' ').trim();
 const label=select.labels?.[0];
 if(label){const copy=label.cloneNode(true);copy.querySelectorAll('select,button,.bo-select').forEach(n=>n.remove());if(copy.textContent.trim())return copy.textContent.trim();}
 return select.title||'选择选项';
}
function optionsOf(select){return [...select.options].map((option,index)=>({index,value:option.value,label:option.dataset.displayLabel||option.label,meta:option.dataset.meta||'',disabled:option.disabled||option.parentElement?.disabled===true,hidden:option.hidden||option.parentElement?.hidden===true,group:option.parentElement?.tagName==='OPTGROUP'?option.parentElement.label:''}));}

// Native selects retain IDs, values, validation and existing event bindings.
// The enhancement owns only presentation, focus and the temporary listbox.
export function installSelects(root=document){
 const controls=new Map();let active=null,frame=0,id=0;
 const measure=document.createElement('canvas').getContext('2d');
 const supportsPopover=typeof HTMLElement.prototype.showPopover==='function';
 function schedule(){if(!frame)frame=requestAnimationFrame(refresh);}
 function enhance(select){
  if(controls.has(select)||select.multiple||select.size>1||select.hidden||select.dataset.nativeSelect!==undefined||select.closest('.runtime-control-bank')||!visible(select))return;
  // Older WebViews keep the styled native control rather than a clipped popup.
  if(!supportsPopover)return;
  const wrapper=document.createElement('span');wrapper.className='bo-select';
  const button=document.createElement('button');button.type='button';button.className='bo-select-trigger';button.setAttribute('role','combobox');button.setAttribute('aria-haspopup','listbox');button.setAttribute('aria-expanded','false');
  const label=document.createElement('span');label.className='bo-select-value';button.append(label);button.insertAdjacentHTML('beforeend',chevron);
  select.before(wrapper);wrapper.append(select,button);select.classList.add('bo-select-native');select.tabIndex=-1;select.setAttribute('aria-hidden','true');
  const control={select,wrapper,button,label,id:`bo-select-${++id}`,name:'',options:[],signature:''};controls.set(select,control);
  button.id=control.id;button.onclick=()=>active?.control===control?close(true):open(control);
  button.addEventListener('keydown',event=>keyDown(event,control));
  select.addEventListener('invalid',event=>{event.preventDefault();button.setAttribute('aria-invalid','true');button.title=select.validationMessage;button.focus();});
  update(control);
 }
 function update(control){
  const {select,wrapper,button,label}=control;
  if(wrapper.hidden!==select.hidden)wrapper.hidden=select.hidden;
  const disabled=select.matches(':disabled')||!select.options.length;if(button.disabled!==disabled)button.disabled=disabled;control.name=selectLabel(select);setAttribute(button,'aria-label',control.name);
  setAttribute(button,'aria-required',select.required);setAttribute(button,'aria-disabled',button.disabled);
  const description=select.getAttribute('aria-describedby');if(description)setAttribute(button,'aria-describedby',description);else button.removeAttribute('aria-describedby');
  const selected=select.selectedOptions[0];const text=selected?.dataset.displayLabel||selected?.label||select.dataset.placeholder||(select.options.length?'请选择':'暂无可选项');setText(label,text);button.title=text;
  if(select.validity.valid)button.removeAttribute('aria-invalid');
  const options=optionsOf(select),signature=JSON.stringify([select.selectedIndex,options]);
  if(control.signature!==signature){control.signature=signature;control.options=options;if(active?.control===control){renderOptions();position();}}
  if(active?.control===control&&button.disabled)close(false);
 }
 function refresh(){
  frame=0;
  for(const [select,control]of controls){if(!select.isConnected){if(active?.control===control)close(false);controls.delete(select);}else update(control);}
  root.querySelectorAll('select').forEach(enhance);
  if(active&&!visible(active.control.button))close(false);
 }
 function focusControl(control){
  if(visible(control.button)){control.button.focus({preventScroll:true});return;}
  // A changed option can replace its complete form section synchronously.
  refresh();const source=control.select;
  const replacement=[...controls.values()].find(c=>visible(c.button)&&(source.id?c.select.id===source.id:source.dataset.setting?c.select.dataset.setting===source.dataset.setting:c.name===control.name));
  replacement?.button.focus({preventScroll:true});
 }
 function close(restore=false){
  if(!active)return;const {control,popup}=active;active=null;
  control.button.setAttribute('aria-expanded','false');control.button.removeAttribute('aria-activedescendant');control.button.removeAttribute('aria-controls');
  if(popup.matches(':popover-open'))popup.hidePopover();popup.remove();
  if(restore)focusControl(control);
 }
 function open(control,typed=''){
  if(control.button.disabled)return;close(false);update(control);
  const searchable=control.options.filter(o=>!o.hidden).length>8||control.select.dataset.searchable==='true';
  const popup=document.createElement('div');popup.className='bo-select-popup';popup.popover='manual';popup.id=`${control.id}-popup`;
  const list=document.createElement('div');list.className='bo-select-list';list.id=`${control.id}-list`;list.setAttribute('role','listbox');list.setAttribute('aria-label',control.name);
  const status=document.createElement('div');status.className='bo-select-status';status.setAttribute('role','status');status.setAttribute('aria-live','polite');
  let input;
  if(searchable){const search=document.createElement('div');search.className='bo-select-search';search.innerHTML=searchIcon;input=document.createElement('input');input.type='text';input.autocomplete='off';input.spellcheck=false;input.placeholder=control.select.id==='activity-event-select'?'搜索活动名称 / ID':'搜索选项';input.setAttribute('aria-label',`搜索${control.name}`);input.setAttribute('role','combobox');input.setAttribute('aria-expanded','true');input.setAttribute('aria-controls',list.id);input.setAttribute('aria-autocomplete','list');search.append(input);popup.append(search);input.addEventListener('input',()=>{if(active){active.query=input.value;active.index=-1;renderOptions();position();list.scrollTop=0;}});input.addEventListener('keydown',e=>keyDown(e,control));}
  popup.append(list,status);(control.select.closest('dialog[open]')||document.body).append(popup);
  active={control,popup,list,input,status,query:searchable?typed:'',rows:[],index:-1,typeahead:'',typedAt:0};
  control.button.setAttribute('aria-expanded','true');control.button.setAttribute('aria-controls',list.id);
  if(input)input.value=typed;
  popup.showPopover();renderOptions();position();setActive(active.index);
  if(input)input.focus({preventScroll:true});else control.button.focus({preventScroll:true});
  popup.addEventListener('pointerdown',e=>{if(e.target.closest('[role=option]'))e.preventDefault();});
  popup.addEventListener('click',e=>{const row=e.target.closest('[role=option]');if(row&&active)choose(Number(row.dataset.index));});
  popup.addEventListener('pointermove',e=>{const row=e.target.closest('[role=option]');if(row&&active&&!active.rows[Number(row.dataset.index)]?.disabled)setActive(Number(row.dataset.index),false);});
 }
 function renderOptions(){
  const state=active;if(!state)return;const {control,list,input,status}=state;
  const previous=state.rows[state.index]?.value;
  state.rows=searchOptions(control.options,state.query);list.replaceChildren();
  let lastGroup='';const fragment=document.createDocumentFragment();
  state.rows.forEach((option,index)=>{
   if(option.group&&option.group!==lastGroup){lastGroup=option.group;const heading=document.createElement('div');heading.className='bo-select-group';heading.textContent=lastGroup;heading.setAttribute('role','presentation');fragment.append(heading);}
   const row=document.createElement('div');row.className='bo-select-option';row.id=`${control.id}-option-${option.index}`;row.dataset.index=index;row.setAttribute('role','option');row.setAttribute('aria-selected',String(option.index===control.select.selectedIndex));row.setAttribute('aria-disabled',String(option.disabled));
   const text=document.createElement('span');text.className='bo-select-option-text';
   const event=control.select.id==='activity-event-select'?option.label.match(/^#?(\d+)\s*·\s*(.+)$/):null;
   if(event){const number=document.createElement('span');number.className='bo-select-id';number.textContent='#'+event[1];row.append(number);text.textContent=event[2];row.classList.add('has-id');}else text.textContent=option.label;
   row.append(text);
   if(option.meta){const meta=document.createElement('span');meta.className='bo-select-option-meta';meta.textContent=option.meta;row.append(meta);row.classList.add('has-meta');}
   const mark=document.createElement('span');mark.className='bo-select-check';mark.innerHTML=check;mark.setAttribute('aria-hidden','true');row.append(mark);fragment.append(row);
  });list.append(fragment);
  if(!state.rows.length){const empty=document.createElement('p');empty.className='bo-select-empty';empty.textContent='没有匹配的选项，试试其他名称或 ID。';list.append(empty);}
  status.hidden=!input;setText(status,state.query?`${state.rows.length} 项匹配`:`${state.rows.length} 个选项 · 可输入名称或 ID`);
  let index=state.rows.findIndex(o=>o.value===(state.query?previous:control.select.value)&&!o.disabled);
  if(index<0)index=moveOption(state.rows,-1,1);setActive(index,false);
 }
 function setActive(index,scroll=true){
  if(!active)return;const {list,input,control}=active;
  if(active.index===index&&list.querySelector(`[data-index="${index}"][data-active=true]`)){if(scroll)list.querySelector(`[data-index="${index}"]`).scrollIntoView({block:'nearest'});return;}
  active.index=index;
  for(const row of list.querySelectorAll('[data-index]'))row.dataset.active=String(Number(row.dataset.index)===index);
  const row=list.querySelector(`[data-index="${index}"]`),target=input||control.button;
  if(row){target.setAttribute('aria-activedescendant',row.id);if(scroll)row.scrollIntoView({block:'nearest'});}else target.removeAttribute('aria-activedescendant');
 }
 function choose(index){
  if(!active)return;const {control,rows}=active,option=rows[index];if(!option||option.disabled)return;
  const {select}=control,changed=select.selectedIndex!==option.index;close(false);
  if(changed){select.selectedIndex=option.index;select.dispatchEvent(new Event('input',{bubbles:true}));select.dispatchEvent(new Event('change',{bubbles:true}));}
  refresh();requestAnimationFrame(()=>focusControl(control));
 }
 function position(){
  if(!active)return;const {control,popup,list,input}=active,anchor=control.button.getBoundingClientRect();
  const v=window.visualViewport,viewport={left:v?.offsetLeft||0,top:v?.offsetTop||0,width:v?.width||document.documentElement.clientWidth,height:v?.height||window.innerHeight};
  const font=getComputedStyle(popup);if(measure)measure.font=`${font.fontWeight} ${font.fontSize} ${font.fontFamily}`;
  const labelWidth=Math.max(0,...control.options.map(o=>measure?.measureText(o.label).width||o.label.length*13));
  const preferredWidth=input?360:Math.min(360,Math.max(180,Math.ceil(labelWidth+76)));
  const width=placeSelectPopup(anchor,viewport,{preferredWidth}).width;
  popup.style.width=width+'px';
  const header=input?popup.querySelector('.bo-select-search').offsetHeight+4+active.status.offsetHeight:0;
  const desired=Math.min(410,list.scrollHeight+header+12);const layout=placeSelectPopup(anchor,viewport,{preferredWidth,desiredHeight:desired,side:active.side});
  Object.assign(popup.style,{left:layout.left+'px',top:layout.top+'px',width:layout.width+'px',maxHeight:layout.maxHeight+'px'});active.side=popup.dataset.side=layout.upward?'above':'below';
 }
 function nextFocus(control,backward){
  const scope=control.select.closest('dialog[open]')||document;
  const nodes=[...scope.querySelectorAll('a[href],button,input,select,textarea,[tabindex]')].filter(n=>n.tabIndex>=0&&!n.matches(':disabled')&&!n.closest('[inert]')&&visible(n));
  const index=nodes.indexOf(control.button),next=nodes[index+(backward?-1:1)];(next||control.button).focus();
 }
 function keyDown(event,control){
  if(event.isComposing||event.key==='Process'||event.ctrlKey||event.metaKey)return;
  if(!active||active.control!==control){
   if(['ArrowDown','ArrowUp','Enter',' '].includes(event.key)){event.preventDefault();open(control);}
   else if(event.key.length===1&&!event.altKey){event.preventDefault();open(control,control.options.length>8?event.key:'');if(active&&!active.input)typeAhead(event.key);}
   return;
  }
  if(event.key==='Escape'||event.altKey&&event.key==='ArrowUp'){event.preventDefault();event.stopPropagation();close(true);return;}
  if(event.key==='Tab'){event.preventDefault();close(false);nextFocus(control,event.shiftKey);return;}
  if(event.key==='Enter'||event.key===' '&&!active.input){event.preventDefault();choose(active.index);return;}
  const offsets={ArrowDown:1,ArrowUp:-1,PageDown:8,PageUp:-8,Home:-Infinity,End:Infinity};
  if(event.key in offsets&&(!active.input||!['Home','End'].includes(event.key))){event.preventDefault();setActive(moveOption(active.rows,active.index,offsets[event.key]));return;}
  if(!active.input&&event.key.length===1&&!event.altKey){event.preventDefault();typeAhead(event.key);}
 }
 function typeAhead(character){
  const now=Date.now();active.typeahead=(now-active.typedAt>700?'':active.typeahead)+character.toLocaleLowerCase();active.typedAt=now;
  const index=active.rows.findIndex(o=>!o.disabled&&o.label.toLocaleLowerCase().startsWith(active.typeahead));if(index>=0)setActive(index);
 }
 const observer=new MutationObserver(records=>{if(records.some(record=>!record.target.closest?.('.bo-select-popup,.bo-select-trigger')))schedule();});
 observer.observe(document.body,{subtree:true,childList:true,characterData:true,attributes:true,attributeFilter:['hidden','disabled','selected','label','data-display-label','data-meta','aria-label','aria-describedby','required','open']});
 document.addEventListener('input',schedule,true);document.addEventListener('change',schedule,true);
 document.addEventListener('reset',()=>setTimeout(schedule,0),true);
 document.addEventListener('pointerdown',event=>{if(active&&!active.popup.contains(event.target)&&!active.control.wrapper.contains(event.target))close(false);},true);
 document.addEventListener('focusin',event=>{if(active&&!active.popup.contains(event.target)&&!active.control.wrapper.contains(event.target))close(false);},true);
 document.addEventListener('keydown',event=>{if(active&&event.key==='Escape'){event.preventDefault();event.stopImmediatePropagation();close(true);}},true);
 document.addEventListener('scroll',event=>{if(active&&!active.popup.contains(event.target)){const rect=active.control.button.getBoundingClientRect();if(rect.bottom<0||rect.top>window.innerHeight)close(false);else{active.side=undefined;position();}}},true);
 document.addEventListener('close',event=>{if(active&&event.target.contains(active.control.select))close(false);},true);
 const reposition=()=>{if(active){active.side=undefined;position();}};
 window.addEventListener('resize',()=>{reposition();schedule();});
 window.visualViewport?.addEventListener('resize',reposition);window.visualViewport?.addEventListener('scroll',reposition);
 refresh();return {refresh,close};
}
