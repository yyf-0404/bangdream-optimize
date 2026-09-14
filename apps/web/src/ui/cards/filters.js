import {bandOrder,bandNames,attributeNames,serverNames,bandSelection,toggleBand,filterAllSelected,toggleFilterSelection} from './rules.js';
import {assetOriginUrl} from '../../assets/index.js';
const iconOrigin=assetOriginUrl()+'/res/icon/';
const ownershipNames={owned:'已持有',missing:'未持有'};
function sharedEl(tag,cls,text){const n=document.createElement(tag);if(cls)n.className=cls;if(text!==undefined)n.textContent=text;return n;}
function sharedImg(src,cls,alt=''){const n=sharedEl('img',cls);n.src=src;n.alt=alt;n.loading='lazy';n.onerror=()=>{n.style.visibility='hidden';};return n;}
function ownershipIcon(name){const span=sharedEl('span','ownership-indicator');span.textContent=name==='check'?'✓':'○';span.setAttribute('aria-hidden','true');return span;}
export function filterValues(characters){return {character:characters.map(c=>String(c.id)),attribute:Object.keys(attributeNames),rarity:['5','4','3','2','1'],server:Object.keys(serverNames),ownership:['owned','missing']};}
export function createFilterControls(root,{characters,getFilters,onChange,onReset,hosts,actionHosts={}}){
 root._filtersController?.abort();root._filtersController=new AbortController();
 const allValues=filterValues(characters);
 const labels={character:'角色',attribute:'属性',rarity:'稀有度',server:'服务器',ownership:'持有状态'};
function checkboxButton(label,cls){
  const b=sharedEl('button',cls);b.type='button';b.role='checkbox';b.setAttribute('aria-label',label);b.title=label;return b;
}
function build(){
  [...new Set([...bandOrder,...characters.map(c=>c.band)])].forEach(id=>{
    const row=sharedEl('div','band-row');row.dataset.band=id;
    const members=characters.filter(ch=>ch.band===id);
    if(!members.length)return;
    const band=checkboxButton(`批量选择 ${bandNames[id]} 的五位成员`,'band-button');band.dataset.band=id;
    band.append(sharedImg(iconOrigin+`band_${id}.svg`));
    band.addEventListener('click',()=>{getFilters().character=toggleBand(getFilters().character,members);onChange();});
    const host=sharedEl('div','members');
    members.forEach(ch=>{
      const b=checkboxButton(ch.name,'character-button');b.dataset.character=ch.id;
      b.append(sharedImg(iconOrigin+`chara_icon_${ch.id}.png`));
      b.addEventListener('click',()=>toggleOption('character',String(ch.id)));host.append(b);
    });
    row.append(band,host);hosts.character.append(row);
  });
  for(const key of ['attribute','rarity','server','ownership']){
    if(!hosts[key])continue;
    allValues[key].forEach(value=>{
      const label=key==='rarity'?`${value} 星`:key==='attribute'?attributeNames[value]:key==='server'?serverNames[value]:ownershipNames[value];
      const b=checkboxButton(label,'filter-option');b.dataset.key=key;b.dataset.value=value;
      if(key==='ownership'){const icon=ownershipIcon(value==='owned'?'check':'circle');icon.classList.add('ownership-indicator');b.append(icon,sharedEl('span','',label));}
      else b.append(sharedImg(iconOrigin+(key==='rarity'?`star_${value}.png`:`${value}.svg`)));
      b.addEventListener('click',()=>toggleOption(key,value));hosts[key].append(b);
    });
  }
  if(hosts.server)hosts.server.title='按发布服务器筛选展示（沿用图鉴未来 7 天窗口）；多选取并集，不限制持有与计算';
  for(const [key,host] of Object.entries(actionHosts)){
    host.classList.add('filter-actions');
    const b=sharedEl('button','filter-action');b.type='button';b.dataset.filterToggle=key;
    host.replaceChildren(b);
    if(key!=='character')hosts[key]?.append(host);
  }
}
function toggleOption(key,value){const set=getFilters()[key];set.has(value)?set.delete(value):set.add(value);onChange();}
function update(){
  root.querySelectorAll('.character-button').forEach(b=>b.setAttribute('aria-checked',String(getFilters().character.has(b.dataset.character))));
  root.querySelectorAll('.band-button').forEach(b=>{
    const members=characters.filter(ch=>ch.band===Number(b.dataset.band)),value=bandSelection(getFilters().character,members);
    b.setAttribute('aria-checked',value);
    b.title=`${bandNames[b.dataset.band]} · ${value==='true'?'全部已选，点击取消全部':value==='mixed'?'部分已选，点击选择全部':'未选择，点击选择全部'}`;
  });
  root.querySelectorAll('.filter-option').forEach(b=>b.setAttribute('aria-checked',String(getFilters()[b.dataset.key].has(b.dataset.value))));
  root.querySelectorAll('[data-filter-toggle]').forEach(b=>{
    const key=b.dataset.filterToggle,all=filterAllSelected(getFilters()[key],allValues[key]);
    b.textContent='全选';b.dataset.complete=String(all);
    b.setAttribute('aria-label','全选'+labels[key]+'筛选');
    b.setAttribute('aria-pressed',String(all));
    b.title=(all?'已全部选择，点击清除':'点击选择全部')+labels[key]+'筛选';
  });
  root.querySelectorAll('[data-filter-count]').forEach(n=>{const key=n.dataset.filterCount;n.textContent=`${getFilters()[key].size} / ${allValues[key].length}`;});
}

 root.addEventListener('click',event=>{
  const button=event.target.closest('button[data-filter-toggle],button[data-filter-reset]');
  if(!button||!root.contains(button))return;
  if(button.hasAttribute('data-filter-reset')){onReset?.();return;}
  const key=button.dataset.filterToggle;if(!allValues[key])return;
  getFilters()[key]=toggleFilterSelection(getFilters()[key],allValues[key]);onChange();
 },{signal:root._filtersController.signal});
 build();update();return {update};
}
